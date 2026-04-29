#!/usr/bin/env python3
import argparse
import ast
import json
import math
import os
import shutil
import subprocess
import tarfile
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

try:
    import tomllib  # Python 3.11+
except ModuleNotFoundError:
    try:
        import tomli as tomllib  # Python <= 3.10
    except ModuleNotFoundError:
        tomllib = None


CRATES_API = "https://crates.io/api/v1/crates"
CRATES_API_PER_PAGE = 100
DEFAULT_TOP_N = 1000


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def run_timestamp_for_filename(run_started_at: str) -> str:
    return (
        run_started_at.replace("+00:00", "Z")
        .replace(":", "")
        .replace("-", "")
    )


def format_duration(seconds: float) -> str:
    total = max(0, int(round(seconds)))
    hours, remainder = divmod(total, 3600)
    minutes, secs = divmod(remainder, 60)
    return f"{hours:02d}:{minutes:02d}:{secs:02d}"


def _truncate_for_table(value: Any, width: int) -> str:
    text = str(value)
    if len(text) <= width:
        return text
    if width <= 3:
        return text[:width]
    return text[: width - 3] + "..."


def summarize_log_tail(text: str, max_lines: int = 8) -> str:
    lines = [line.rstrip() for line in text.splitlines() if line.strip()]
    if not lines:
        return ""
    return "\n".join(lines[-max_lines:])


def build_run_report(
    *,
    run_started_at: str,
    run_finished_at: str,
    args: argparse.Namespace,
    source_mode: str,
    dataset_path: Path,
    dataset_index_path: Path,
    snapshot_path: Path,
    crates: List[Dict[str, Any]],
    crate_summaries: List[Dict[str, Any]],
    total_bc_count: int,
    matched_bc_count: int,
    retained_bc_count: int,
    success_count: int,
    failed_count: int,
    overall_status: str,
    error: Optional[str] = None,
) -> str:
    started_dt = datetime.fromisoformat(run_started_at)
    finished_dt = datetime.fromisoformat(run_finished_at)
    elapsed = format_duration((finished_dt - started_dt).total_seconds())

    status_buckets: Dict[str, int] = {}
    for item in crate_summaries:
        status = str(item.get("status", "unknown"))
        status_buckets[status] = status_buckets.get(status, 0) + 1

    crate_table_columns = [
        ("crate", 18),
        ("version", 14),
        ("rank", 6),
        ("status", 18),
        ("toolchain", 18),
        ("elapsed", 10),
        ("bc", 6),
        ("matched", 8),
        ("retained", 9),
        ("origin", 16),
        ("status_file", 48),
    ]

    lines = [
        "RAPx Bounds-Check Dataset Run Report",
        f"started_at: {run_started_at}",
        f"finished_at: {run_finished_at}",
        f"elapsed: {elapsed}",
        f"overall_status: {overall_status}",
        "",
        "Configuration",
        f"- source_mode: {source_mode}",
        f"- offline: {args.offline}",
        f"- top_n: {args.top_n}",
        f"- crates_file: {args.crates_file if args.crates_file is not None else 'null'}",
        f"- sources_dir: {args.output_dir.resolve() / 'sources'}",
        f"- toolchain: {args.toolchain}",
        f"- timeout_sec: {args.timeout_sec}",
        f"- output_dir: {args.output_dir.resolve()}",
        f"- snapshot_path: {snapshot_path if source_mode == 'popular_snapshot' else 'null'}",
        f"- dataset_path: {dataset_path}",
        f"- dataset_index_path: {dataset_index_path}",
        "",
        "Aggregate Metrics",
        f"- requested_crates: {len(crates)}",
        f"- success_count: {success_count}",
        f"- failed_count: {failed_count}",
        f"- total_bc_count: {total_bc_count}",
        f"- matched_bc_count: {matched_bc_count}",
        f"- retained_bc_count: {retained_bc_count}",
        (
            f"- matched_ratio: {matched_bc_count}/{total_bc_count}"
            if total_bc_count
            else "- matched_ratio: 0/0"
        ),
        (
            f"- retained_ratio: {retained_bc_count}/{total_bc_count}"
            if total_bc_count
            else "- retained_ratio: 0/0"
        ),
    ]

    if error:
        lines.extend(["", "Error", f"- {error}"])

    if status_buckets:
        lines.extend(["", "Crate Status Counts"])
        for status in sorted(status_buckets):
            lines.append(f"- {status}: {status_buckets[status]}")

    if crate_summaries:
        lines.extend(["", "Per-Crate Summary"])
        header = " ".join(name.ljust(width) for name, width in crate_table_columns)
        divider = " ".join("-" * width for _, width in crate_table_columns)
        lines.append(header)
        lines.append(divider)
        for item in crate_summaries:
            row_values = {
                "crate": item.get("crate", "<unknown>"),
                "version": item.get("version", "<unknown>"),
                "rank": item.get("rank", "null"),
                "status": item.get("status", "unknown"),
                "toolchain": item.get("used_toolchain", "null"),
                "elapsed": item.get("elapsed_text", "null"),
                "bc": item.get("bc_count", "null"),
                "matched": item.get("matched_rows", "null"),
                "retained": item.get("retained_rows", "null"),
                "origin": item.get("source_origin", "null"),
                "status_file": item.get("crate_status_path", ""),
            }
            line = " ".join(
                _truncate_for_table(row_values[name], width).ljust(width)
                for name, width in crate_table_columns
            )
            lines.append(line)

    success_statuses = {"ok", "ok_with_rapx_nonzero"}
    failures = [item for item in crate_summaries if item.get("status") not in success_statuses]
    if failures:
        lines.extend(["", "Failures Detail"])
        for item in failures:
            lines.append(
                f"- {item.get('crate', '<unknown>')}@{item.get('version', '<unknown>')} "
                f"status={item.get('status', 'unknown')} "
                f"toolchain={item.get('used_toolchain', 'null')} "
                f"last_cmd={item.get('last_attempt_command', 'null')} "
                f"error={item.get('error', 'null')} "
                f"status_file={item.get('crate_status_path', '')}"
            )
            log_tail = item.get("log_tail")
            if log_tail:
                for line in str(log_tail).splitlines():
                    lines.append(f"    {line}")

    return "\n".join(lines) + "\n"


def _extract_toml_section_value(text: str, section: str, key: str) -> Any:
    current_section: Optional[str] = None
    for raw_line in text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            current_section = line[1:-1].strip()
            continue
        if current_section != section or "=" not in line:
            continue
        lhs, rhs = line.split("=", 1)
        if lhs.strip() != key:
            continue
        value = rhs.strip()
        try:
            return ast.literal_eval(value)
        except Exception:
            return value.strip("\"'")
    return None


def _load_toml_payload(text: str) -> Dict[str, Any]:
    if tomllib is not None:
        return tomllib.loads(text)

    payload: Dict[str, Any] = {}
    toolchain_channel = _extract_toml_section_value(text, "toolchain", "channel")
    package_name = _extract_toml_section_value(text, "package", "name")
    package_version = _extract_toml_section_value(text, "package", "version")
    package_rust_version = _extract_toml_section_value(text, "package", "rust-version")
    workspace_members = _extract_toml_section_value(text, "workspace", "members")

    if toolchain_channel is not None:
        payload["toolchain"] = {"channel": toolchain_channel}
    if package_name is not None or package_version is not None or package_rust_version is not None:
        payload["package"] = {}
        if package_name is not None:
            payload["package"]["name"] = package_name
        if package_version is not None:
            payload["package"]["version"] = package_version
        if package_rust_version is not None:
            payload["package"]["rust-version"] = package_rust_version
    if workspace_members is not None:
        payload["workspace"] = {"members": workspace_members}
    return payload


def fetch_popular_crates(limit: int) -> List[Dict[str, Any]]:
    limit = max(limit, 1)
    total_pages = math.ceil(limit / CRATES_API_PER_PAGE)
    fetched_at = utc_now_iso()
    result: List[Dict[str, Any]] = []

    for page in range(1, total_pages + 1):
        per_page = min(CRATES_API_PER_PAGE, limit - len(result))
        url = f"{CRATES_API}?sort=downloads&page={page}&per_page={per_page}"
        req = urllib.request.Request(url, headers={"User-Agent": "rapx-bc-dataset-script"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        crates = payload.get("crates", [])
        for item in crates:
            version = (
                item.get("max_stable_version")
                or item.get("newest_version")
                or item.get("max_version")
                or ""
            )
            name = item.get("id")
            if not name or not version:
                continue
            result.append(
                {
                    "rank": len(result) + 1,
                    "name": name,
                    "version": version,
                    "downloads": item.get("downloads"),
                    "fetched_at": fetched_at,
                }
            )
            if len(result) >= limit:
                break
    return result[:limit]


def _normalize_crate_entry(entry: Any) -> Optional[Dict[str, Any]]:
    if isinstance(entry, dict):
        name = str(entry.get("name", "")).strip()
        version = str(entry.get("version", "")).strip()
        if not name or not version:
            return None
        normalized: Dict[str, Any] = {"name": name, "version": version}
        if "rank" in entry:
            normalized["rank"] = entry.get("rank")
        if "downloads" in entry:
            normalized["downloads"] = entry.get("downloads")
        if "fetched_at" in entry:
            normalized["fetched_at"] = entry.get("fetched_at")
        return normalized

    if isinstance(entry, str):
        text = entry.strip()
        if not text:
            return None
        if "@" in text:
            name, version = text.split("@", 1)
            name = name.strip()
            version = version.strip()
            if name and version:
                return {"name": name, "version": version}
        parts = text.split()
        if len(parts) >= 2:
            name = parts[0].strip()
            version = parts[1].strip()
            if name and version:
                return {"name": name, "version": version}
    return None


def load_fixed_crates(crates_file: Path) -> List[Dict[str, Any]]:
    if not crates_file.exists():
        raise FileNotFoundError(f"crates file not found: {crates_file}")

    text = crates_file.read_text(encoding="utf-8")

    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        payload = None

    crates: List[Dict[str, Any]] = []
    if isinstance(payload, list):
        for item in payload:
            normalized = _normalize_crate_entry(item)
            if normalized:
                crates.append(normalized)
    else:
        for raw_line in text.splitlines():
            line = raw_line.split("#", 1)[0].strip()
            if not line:
                continue
            normalized = _normalize_crate_entry(line)
            if normalized:
                crates.append(normalized)

    if not crates:
        raise ValueError(
            "no valid crate entries found; expected lines like 'name@version' or JSON list"
        )
    return crates


def discover_crates_from_sources(sources_dir: Path) -> List[Dict[str, Any]]:
    if not sources_dir.exists():
        raise FileNotFoundError(f"sources directory not found: {sources_dir}")

    discovered: List[Dict[str, Any]] = []
    seen = set()
    for entry in sorted(sources_dir.iterdir(), key=lambda p: p.name):
        if not entry.is_dir():
            continue
        crate_root = entry
        cargo_toml = crate_root / "Cargo.toml"
        if not cargo_toml.exists():
            subdirs = [p for p in crate_root.iterdir() if p.is_dir()]
            if len(subdirs) == 1 and (subdirs[0] / "Cargo.toml").exists():
                crate_root = subdirs[0]
                cargo_toml = crate_root / "Cargo.toml"
            else:
                continue
        try:
            payload = _load_toml_payload(cargo_toml.read_text(encoding="utf-8"))
        except Exception:
            continue
        package = payload.get("package")
        if not isinstance(package, dict):
            continue
        name = str(package.get("name", "")).strip()
        version = str(package.get("version", "")).strip()
        if not name or not version:
            continue
        key = (name, version)
        if key in seen:
            continue
        seen.add(key)
        discovered.append({"name": name, "version": version})

    if not discovered:
        raise ValueError(f"no valid crates discovered under sources directory: {sources_dir}")
    return discovered


def download_and_extract_crate(crate: str, version: str, dst_dir: Path) -> Path:
    dst_dir.mkdir(parents=True, exist_ok=True)
    crate_workdir = dst_dir / f"{crate}-{version}"
    if crate_workdir.exists():
        shutil.rmtree(crate_workdir)
    crate_workdir.mkdir(parents=True, exist_ok=True)

    tar_path = crate_workdir / "crate.tar.gz"
    url = f"https://crates.io/api/v1/crates/{crate}/{version}/download"
    req = urllib.request.Request(url, headers={"User-Agent": "rapx-bc-dataset-script"})
    with urllib.request.urlopen(req, timeout=60) as resp, tar_path.open("wb") as f:
        f.write(resp.read())

    with tarfile.open(tar_path, "r:gz") as tar:
        for member in tar.getmembers():
            member_path = (crate_workdir / member.name).resolve()
            if not str(member_path).startswith(str(crate_workdir.resolve())):
                raise ValueError(f"Refusing to extract unsafe tar entry: {member.name}")
        tar.extractall(crate_workdir)
    tar_path.unlink(missing_ok=True)

    subdirs = [p for p in crate_workdir.iterdir() if p.is_dir()]
    if len(subdirs) == 1:
        return subdirs[0]
    return crate_workdir


def resolve_existing_staged_crate_source(crate: str, version: str, sources_dir: Path) -> Path:
    candidates = [
        sources_dir / f"{crate}-{version}",
        sources_dir / crate / version,
        sources_dir / crate,
    ]
    for candidate in candidates:
        if (candidate / "Cargo.toml").exists():
            return candidate
        subdirs = [p for p in candidate.iterdir() if p.is_dir()] if candidate.exists() else []
        if len(subdirs) == 1 and (subdirs[0] / "Cargo.toml").exists():
            return subdirs[0]
    raise FileNotFoundError(
        f"existing crate source not found for {crate}@{version} under {sources_dir}"
    )


def _normalize_toolchain(channel: Any) -> Optional[str]:
    if not isinstance(channel, str):
        return None
    cleaned = channel.strip()
    if not cleaned:
        return None
    normalized = cleaned[1:] if cleaned.startswith("+") else cleaned
    parts = normalized.split(".")
    if len(parts) == 2 and all(part.isdigit() for part in parts):
        return f"{normalized}.0"
    return normalized


def _detect_crate_toolchain(crate_dir: Path) -> Optional[str]:
    rust_toolchain_toml = crate_dir / "rust-toolchain.toml"
    if rust_toolchain_toml.exists():
        try:
            payload = _load_toml_payload(rust_toolchain_toml.read_text(encoding="utf-8"))
            normalized = _normalize_toolchain(payload.get("toolchain", {}).get("channel"))
            if normalized:
                return normalized
        except Exception:
            pass

    rust_toolchain = crate_dir / "rust-toolchain"
    if rust_toolchain.exists():
        content = rust_toolchain.read_text(encoding="utf-8").strip()
        first_value: Optional[str] = None
        for line in content.splitlines():
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            first_value = stripped.split("#", 1)[0].strip()
            break
        if first_value and "=" not in first_value and not first_value.startswith("["):
            normalized = _normalize_toolchain(first_value)
            if normalized and not any(ch.isspace() for ch in normalized):
                return normalized
        try:
            payload = _load_toml_payload(content)
            normalized = _normalize_toolchain(payload.get("toolchain", {}).get("channel"))
            if normalized:
                return normalized
        except Exception:
            pass

    cargo_toml = crate_dir / "Cargo.toml"
    if not cargo_toml.exists():
        return None
    try:
        payload = _load_toml_payload(cargo_toml.read_text(encoding="utf-8"))
        rust_version = payload.get("package", {}).get("rust-version")
        return _normalize_toolchain(rust_version)
    except Exception:
        return None


def crate_uses_workspace_members(crate_dir: Path) -> bool:
    cargo_toml = crate_dir / "Cargo.toml"
    if not cargo_toml.exists():
        return False
    try:
        payload = _load_toml_payload(cargo_toml.read_text(encoding="utf-8"))
    except Exception:
        return False
    workspace = payload.get("workspace")
    members = workspace.get("members") if isinstance(workspace, dict) else None
    return isinstance(members, list) and len(members) > 0


def crate_is_proc_macro(crate_dir: Path) -> bool:
    cargo_toml = crate_dir / "Cargo.toml"
    if not cargo_toml.exists():
        return False
    text = cargo_toml.read_text(encoding="utf-8")
    try:
        payload = _load_toml_payload(text)
    except Exception:
        payload = {}
    in_lib_section = False
    for raw_line in text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            in_lib_section = line[1:-1].strip() == "lib"
            continue
        if in_lib_section and "=" in line:
            lhs, rhs = line.split("=", 1)
            if lhs.strip() == "proc-macro" and rhs.strip().lower() == "true":
                return True
    lib = payload.get("lib")
    if not isinstance(lib, dict):
        return False
    return lib.get("proc-macro") is True


def local_cargo_rapx_dir() -> Optional[Path]:
    candidate = Path(__file__).resolve().parent / "rapx" / "target" / "debug" / "cargo-rapx"
    return candidate.parent if candidate.exists() else None


def run_rapx(
    crate_dir: Path, toolchain: str, timeout_sec: int
) -> Tuple[bool, str, str, float, str, Optional[str], str]:
    attempts: List[Tuple[List[str], str]] = []
    crate_toolchain = _detect_crate_toolchain(crate_dir)
    fallback_toolchain = _normalize_toolchain(toolchain)
    if crate_toolchain:
        attempts.append(
            (["cargo", f"+{crate_toolchain}", "rapx", "-bounds-db"], crate_toolchain)
        )
    if fallback_toolchain:
        attempts.append(
            (
                ["cargo", f"+{fallback_toolchain}", "rapx", "-bounds-db"],
                fallback_toolchain,
            )
        )
    attempts.append((["cargo", "+stable", "rapx", "-bounds-db"], "stable"))
    attempts.append((["cargo", "+nightly", "rapx", "-bounds-db"], "nightly"))
    attempts.append((["cargo", "rapx", "-bounds-db"], "default"))

    rap_recursive = "shallow" if crate_uses_workspace_members(crate_dir) else "none"
    seen = set()
    merged_output: List[str] = []
    started_at = time.monotonic()
    last_cmd_text: Optional[str] = None
    last_output = ""

    for cmd, used_toolchain in attempts:
        cmd_key = tuple(cmd)
        if cmd_key in seen:
            continue
        seen.add(cmd_key)
        env = os.environ.copy()
        env["RAP_RECURSIVE"] = rap_recursive
        local_rapx_dir = local_cargo_rapx_dir()
        if local_rapx_dir is not None:
            env["PATH"] = f"{local_rapx_dir}{os.pathsep}{env.get('PATH', '')}"
        last_cmd_text = " ".join(cmd)
        proc = subprocess.run(
            cmd,
            cwd=crate_dir,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout_sec,
            check=False,
        )
        last_output = proc.stdout
        merged_output.append(
            f"$ {' '.join(cmd)}\n# RAP_RECURSIVE={rap_recursive}\n{proc.stdout}"
        )
        if proc.returncode == 0:
            return (
                True,
                "\n".join(merged_output),
                used_toolchain,
                time.monotonic() - started_at,
                rap_recursive,
                last_cmd_text,
                summarize_log_tail(proc.stdout),
            )

    return (
        False,
        "\n".join(merged_output),
        "none",
        time.monotonic() - started_at,
        rap_recursive,
        last_cmd_text,
        summarize_log_tail(last_output),
    )


def find_latest_bc_json(crate_dir: Path) -> Optional[Path]:
    candidates = sorted(
        crate_dir.rglob("bounds_checks*.json"), key=lambda p: p.stat().st_mtime
    )
    if not candidates:
        candidates = sorted(
            crate_dir.rglob("*bounds*check*.json"), key=lambda p: p.stat().st_mtime
        )
    return candidates[-1] if candidates else None


def _extract_list(payload: Any, keys: List[str]) -> List[Any]:
    cur = payload
    for key in keys:
        if not isinstance(cur, dict):
            return []
        cur = cur.get(key)
    return cur if isinstance(cur, list) else []


def extract_bounds_checks(payload: Dict[str, Any]) -> List[Dict[str, Any]]:
    bcs = _extract_list(payload, ["bounds_checks"])
    if bcs:
        return [x for x in bcs if isinstance(x, dict)]
    if isinstance(payload, list):
        return [x for x in payload if isinstance(x, dict)]
    return []


def extract_reserved_markers(payload: Dict[str, Any]) -> List[Any]:
    llvm_records = _extract_list(payload, ["llvm", "reserved", "records"])
    if llvm_records:
        return llvm_records
    llvm_reserved = _extract_list(payload, ["llvm", "reserved"])
    if llvm_reserved:
        return llvm_reserved
    if "llvm_reserved" in payload and isinstance(payload["llvm_reserved"], list):
        return payload["llvm_reserved"]
    return []


def _first_present(d: Dict[str, Any], keys: List[str]) -> Any:
    for k in keys:
        if k in d:
            return d[k]
    return None


def _to_int(value: Any) -> Optional[int]:
    if isinstance(value, int):
        return value
    if isinstance(value, str) and value.isdigit():
        return int(value)
    return None


def _find_match_by_line_file(bc: Dict[str, Any], markers: List[Any]) -> Optional[Any]:
    location = bc.get("location") if isinstance(bc.get("location"), dict) else {}
    bc_file = _first_present(location, ["file", "filename", "source_file"])
    bc_line = _to_int(_first_present(location, ["line", "line_no", "source_line"]))
    if bc_file is None or bc_line is None:
        bc_file = _first_present(bc, ["file", "filename", "source_file"])
        bc_line = _to_int(_first_present(bc, ["line", "line_no", "source_line"]))
    if bc_file is None or bc_line is None:
        return None

    for marker in markers:
        if not isinstance(marker, dict):
            continue
        m_file = _first_present(marker, ["file", "filename", "source_file"])
        m_line = _to_int(_first_present(marker, ["line", "line_no", "source_line"]))
        if m_file == bc_file and m_line == bc_line:
            return marker
    return None


def _find_match_by_id(bc: Dict[str, Any], markers: List[Any]) -> Optional[Any]:
    bc_id = _first_present(
        bc, ["llvm_reserved_id", "reserved_id", "llvm_id", "marker_id", "id"]
    )
    if bc_id is None and isinstance(bc.get("location"), dict):
        bc_id = _first_present(
            bc["location"], ["llvm_reserved_id", "reserved_id", "llvm_id", "marker_id", "id"]
        )
    if bc_id is None:
        return None

    for marker in markers:
        if isinstance(marker, dict):
            marker_id = _first_present(
                marker, ["llvm_reserved_id", "reserved_id", "llvm_id", "marker_id", "id"]
            )
            if marker_id == bc_id:
                return marker
    return None


def build_dataset_rows(
    crate: str,
    version: str,
    rank: Optional[int],
    json_path: Path,
) -> Tuple[List[Dict[str, Any]], int, int, int]:
    payload = json.loads(json_path.read_text(encoding="utf-8"))
    bcs = extract_bounds_checks(payload)
    markers = extract_reserved_markers(payload)

    rows: List[Dict[str, Any]] = []
    unmatched = 0
    for idx, bc in enumerate(bcs):
        marker = _find_match_by_id(bc, markers)
        if marker is None:
            marker = _find_match_by_line_file(bc, markers)
        matched = marker is not None
        if not matched:
            unmatched += 1
        rows.append(
            {
                "crate": crate,
                "version": version,
                "rank": rank,
                "bc_index": idx,
                "bc": bc,
                "llvm_reserved": marker,
                "llvm_reserved_matched": matched,
                "llvm_retained": (
                    marker.get("retained") if isinstance(marker, dict) else None
                ),
                "raw_json_path": str(json_path),
            }
        )
    return rows, len(bcs), len(markers), unmatched


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def write_run_report(path: Path, content: str) -> None:
    path.write_text(content, encoding="utf-8")


def load_or_create_snapshot(
    *,
    crates_file: Optional[Path],
    snapshot_path: Path,
    top_n: int,
    offline: bool,
    sources_dir: Path,
) -> Tuple[List[Dict[str, Any]], str]:
    if offline:
        if crates_file is not None:
            crates = load_fixed_crates(crates_file)
            return crates, "offline_fixed_crates"
        crates = discover_crates_from_sources(sources_dir)
        return crates, "offline_existing_sources"
    if crates_file is not None:
        crates = load_fixed_crates(crates_file)
        return crates, "fixed_crates"

    crates = fetch_popular_crates(top_n)
    write_json(snapshot_path, crates)
    return crates, "popular_snapshot"


def validate_args(args: argparse.Namespace) -> None:
    if args.local_sources_dir is not None and not args.offline:
        raise ValueError("--local-sources-dir requires --offline")


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(
            "Download crates, run rapx -bounds-db, and build a layered JSON dataset "
            "for bounds checks and LLVM BCE retention."
        )
    )
    parser.add_argument(
        "--top-n",
        type=int,
        default=DEFAULT_TOP_N,
        help="How many popular crates to process.",
    )
    parser.add_argument(
        "--crates-file",
        type=Path,
        default=None,
        help="Path to a fixed crate list file (JSON list or text lines like 'crate@version').",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("dataset_bc"),
        help="Directory to store crate sources, raw JSON, status JSON, and final dataset.",
    )
    parser.add_argument(
        "--toolchain",
        default="nightly-2025-12-06",
        help="Rust toolchain used for cargo +<toolchain> rapx -bounds-db.",
    )
    parser.add_argument(
        "--timeout-sec",
        type=int,
        default=1800,
        help="Timeout (seconds) for each crate analysis.",
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="Analyze existing crate sources under <output-dir>/sources instead of downloading from crates.io.",
    )
    parser.add_argument(
        "--local-sources-dir",
        type=Path,
        default=None,
        help=(
            "Deprecated. Offline mode now reads already prepared sources from <output-dir>/sources."
        ),
    )
    args = parser.parse_args()

    output_dir: Path = args.output_dir.resolve()
    sources_dir = output_dir / "sources"
    raw_json_dir = output_dir / "raw_json"
    logs_dir = output_dir / "logs"
    crate_status_dir = output_dir / "crate_status"
    run_reports_dir = output_dir / "run_reports"
    snapshot_path = output_dir / f"popular_crates_top{args.top_n}.json"
    dataset_path = output_dir / "bounds_checks_dataset.json"
    dataset_index_path = output_dir / "dataset_index.json"

    for d in (sources_dir, raw_json_dir, logs_dir, crate_status_dir, run_reports_dir):
        d.mkdir(parents=True, exist_ok=True)

    run_started_at = utc_now_iso()
    run_report_path = (
        run_reports_dir / f"run_report_{run_timestamp_for_filename(run_started_at)}.log"
    )
    try:
        validate_args(args)
        crates, source_mode = load_or_create_snapshot(
            crates_file=args.crates_file,
            snapshot_path=snapshot_path,
            top_n=args.top_n,
            offline=args.offline,
            sources_dir=sources_dir,
        )
    except Exception as exc:
        run_finished_at = utc_now_iso()
        dataset_index = {
            "run_started_at": run_started_at,
            "run_finished_at": run_finished_at,
            "top_n": args.top_n,
            "crates_file": str(args.crates_file) if args.crates_file is not None else None,
            "snapshot_path": (
                str(snapshot_path) if args.crates_file is None else None
            ),
            "toolchain": args.toolchain,
            "timeout_sec": args.timeout_sec,
            "workspace_strategy": "shallow_for_workspaces",
            "status": (
                "load_fixed_crates_failed"
                if args.crates_file is not None
                else ("load_sources_failed" if args.offline else "fetch_popular_crates_failed")
            ),
            "error": str(exc),
            "source_mode": (
                "offline_fixed_crates"
                if args.offline and args.crates_file is not None
                else (
                    "offline_existing_sources"
                    if args.offline
                    else ("fixed_crates" if args.crates_file is not None else "popular_snapshot")
                )
            ),
            "offline": args.offline,
            "sources_dir": str(sources_dir),
            "run_report_path": str(run_report_path),
            "crate_count": 0,
            "success_count": 0,
            "failed_count": 0,
            "total_bc_count": 0,
            "matched_bc_count": 0,
            "retained_bc_count": 0,
            "crates": [],
        }
        write_json(dataset_index_path, dataset_index)
        run_report = build_run_report(
            run_started_at=run_started_at,
            run_finished_at=run_finished_at,
            args=args,
            source_mode=dataset_index["source_mode"],
            dataset_path=dataset_path,
            dataset_index_path=dataset_index_path,
            snapshot_path=snapshot_path,
            crates=[],
            crate_summaries=[],
            total_bc_count=0,
            matched_bc_count=0,
            retained_bc_count=0,
            success_count=0,
            failed_count=0,
            overall_status=dataset_index["status"],
            error=str(exc),
        )
        write_run_report(run_report_path, run_report)
        print(json.dumps(dataset_index, ensure_ascii=False, indent=2))
        return

    all_records: List[Dict[str, Any]] = []
    crate_summaries: List[Dict[str, Any]] = []
    total_bc_count = 0
    matched_bc_count = 0
    retained_bc_count = 0
    success_count = 0

    for item in crates:
        crate = item["name"]
        version = item["version"]
        rank = item.get("rank")
        status: Dict[str, Any] = {
            "crate": crate,
            "version": version,
            "rank": rank,
            "downloads": item.get("downloads"),
            "fetched_at": item.get("fetched_at"),
            "run_started_at": utc_now_iso(),
        }
        status_path = crate_status_dir / f"{crate}-{version}.json"

        try:
            if args.offline:
                crate_dir = resolve_existing_staged_crate_source(crate, version, sources_dir)
                status["source_origin"] = "existing_sources"
            else:
                crate_dir = download_and_extract_crate(crate, version, sources_dir)
                status["source_origin"] = "downloaded"
            status["source_dir"] = str(crate_dir)

            (
                ok,
                rapx_output,
                used_toolchain,
                elapsed_sec,
                rap_recursive,
                last_attempt_command,
                log_tail,
            ) = run_rapx(
                crate_dir, args.toolchain, args.timeout_sec
            )
            (logs_dir / f"{crate}-{version}.log").write_text(rapx_output, encoding="utf-8")

            status["used_toolchain"] = used_toolchain
            status["elapsed_sec"] = round(elapsed_sec, 3)
            status["rap_recursive"] = rap_recursive
            status["last_attempt_command"] = last_attempt_command
            status["log_tail"] = log_tail if log_tail else None
            status["rapx_exit_ok"] = ok

            json_path = find_latest_bc_json(crate_dir)
            if json_path is None:
                if ok and crate_is_proc_macro(crate_dir):
                    status["status"] = "unsupported_proc_macro_or_no_bounds_json"
                else:
                    status["status"] = "bc_json_not_found" if ok else "rapx_failed"
                status["run_finished_at"] = utc_now_iso()
                write_json(status_path, status)
                crate_summaries.append(
                    {
                        "crate": crate,
                        "version": version,
                        "rank": rank,
                        "status": status["status"],
                        "used_toolchain": status.get("used_toolchain"),
                        "elapsed_text": format_duration(status.get("elapsed_sec", 0.0)),
                        "bc_count": status.get("bc_count"),
                        "matched_rows": status.get("matched_rows"),
                        "retained_rows": status.get("retained_rows"),
                        "source_origin": status.get("source_origin"),
                        "error": status.get("error"),
                        "last_attempt_command": status.get("last_attempt_command"),
                        "log_tail": status.get("log_tail"),
                        "crate_status_path": str(status_path),
                    }
                )
                continue

            copied_json = raw_json_dir / f"{crate}-{version}.json"
            shutil.copy2(json_path, copied_json)
            rows, bc_count, marker_count, unmatched = build_dataset_rows(
                crate, version, rank, copied_json
            )

            status["status"] = "ok" if ok else "ok_with_rapx_nonzero"
            status["raw_json_path"] = str(copied_json)
            status["bc_count"] = bc_count
            status["reserved_count"] = marker_count
            status["dataset_rows"] = len(rows)
            status["unmatched_rows"] = unmatched
            status["matched_rows"] = len(rows) - unmatched
            status["retained_rows"] = sum(
                1 for row in rows if row.get("llvm_retained") is True
            )
            status["run_finished_at"] = utc_now_iso()

            total_bc_count += bc_count
            matched_bc_count += status["matched_rows"]
            retained_bc_count += status["retained_rows"]
            all_records.extend(rows)
            success_count += 1
        except Exception as exc:
            status["status"] = "error"
            status["error"] = str(exc)
            status["run_finished_at"] = utc_now_iso()

        write_json(status_path, status)
        crate_summaries.append(
            {
                "crate": crate,
                "version": version,
                "rank": rank,
                "status": status["status"],
                "used_toolchain": status.get("used_toolchain"),
                "elapsed_text": format_duration(status.get("elapsed_sec", 0.0)),
                "bc_count": status.get("bc_count"),
                "matched_rows": status.get("matched_rows"),
                "retained_rows": status.get("retained_rows"),
                "source_origin": status.get("source_origin"),
                "error": status.get("error"),
                "last_attempt_command": status.get("last_attempt_command"),
                "log_tail": status.get("log_tail"),
                "crate_status_path": str(status_path),
            }
        )

    failed_count = len(crates) - success_count

    run_finished_at = utc_now_iso()
    dataset_payload = {
        "metadata": {
            "run_started_at": run_started_at,
            "run_finished_at": run_finished_at,
            "toolchain": args.toolchain,
            "timeout_sec": args.timeout_sec,
            "workspace_strategy": "shallow_for_workspaces",
            "source_mode": source_mode,
            "offline": args.offline,
            "sources_dir": str(sources_dir),
            "snapshot_path": (
                str(snapshot_path) if source_mode == "popular_snapshot" else None
            ),
            "crates_file": str(args.crates_file) if args.crates_file is not None else None,
            "run_report_path": str(run_report_path),
            "crate_count": len(crates),
            "success_count": success_count,
            "failed_count": failed_count,
            "total_bc_count": total_bc_count,
            "matched_bc_count": matched_bc_count,
            "retained_bc_count": retained_bc_count,
        },
        "crates": crates,
        "records": all_records,
    }
    write_json(dataset_path, dataset_payload)

    dataset_index = {
        "run_started_at": run_started_at,
        "run_finished_at": dataset_payload["metadata"]["run_finished_at"],
        "top_n": args.top_n,
        "toolchain": args.toolchain,
        "timeout_sec": args.timeout_sec,
        "workspace_strategy": "shallow_for_workspaces",
        "source_mode": source_mode,
        "offline": args.offline,
        "sources_dir": str(sources_dir),
        "run_report_path": str(run_report_path),
        "crates_file": str(args.crates_file) if args.crates_file is not None else None,
        "snapshot_path": (
            str(snapshot_path) if source_mode == "popular_snapshot" else None
        ),
        "dataset_path": str(dataset_path),
        "crate_count": len(crates),
        "success_count": success_count,
        "failed_count": failed_count,
        "total_bc_count": total_bc_count,
        "matched_bc_count": matched_bc_count,
        "retained_bc_count": retained_bc_count,
        "status": "ok",
        "crates": crate_summaries,
    }
    write_json(dataset_index_path, dataset_index)
    run_report = build_run_report(
        run_started_at=run_started_at,
        run_finished_at=run_finished_at,
        args=args,
        source_mode=source_mode,
        dataset_path=dataset_path,
        dataset_index_path=dataset_index_path,
        snapshot_path=snapshot_path,
        crates=crates,
        crate_summaries=crate_summaries,
        total_bc_count=total_bc_count,
        matched_bc_count=matched_bc_count,
        retained_bc_count=retained_bc_count,
        success_count=success_count,
        failed_count=failed_count,
        overall_status="ok",
    )
    write_run_report(run_report_path, run_report)
    print(json.dumps(dataset_index, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
