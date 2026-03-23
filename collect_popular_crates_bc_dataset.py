#!/usr/bin/env python3
import argparse
import json
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


CRATES_API = "https://crates.io/api/v1/crates"


def fetch_popular_crates(limit: int) -> List[Dict[str, str]]:
    per_page = min(max(limit, 1), 100)
    url = f"{CRATES_API}?sort=downloads&page=1&per_page={per_page}"
    req = urllib.request.Request(url, headers={"User-Agent": "rapx-bc-dataset-script"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        payload = json.loads(resp.read().decode("utf-8"))
    crates = payload.get("crates", [])
    result: List[Dict[str, str]] = []
    for item in crates[:limit]:
        version = (
            item.get("max_stable_version")
            or item.get("newest_version")
            or item.get("max_version")
            or ""
        )
        if not item.get("id") or not version:
            continue
        result.append({"name": item["id"], "version": version})
    return result


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


def run_rapx(crate_dir: Path, toolchain: str, timeout_sec: int) -> Tuple[bool, str]:
    cmd = ["cargo", f"+{toolchain}", "rapx", "-O", "--", "--locked"]
    proc = subprocess.run(
        cmd,
        cwd=crate_dir,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=timeout_sec,
        check=False,
    )
    if proc.returncode == 0:
        return True, proc.stdout
    fallback_cmd = ["cargo", "rapx", "-O", "--", "--locked"]
    fallback_proc = subprocess.run(
        fallback_cmd,
        cwd=crate_dir,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=timeout_sec,
        check=False,
    )
    return fallback_proc.returncode == 0, fallback_proc.stdout


def find_latest_bc_json(crate_dir: Path) -> Optional[Path]:
    candidates = sorted(crate_dir.rglob("bounds_checks*.json"), key=lambda p: p.stat().st_mtime)
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
    crate: str, version: str, json_path: Path
) -> Tuple[List[Dict[str, Any]], int, int, int]:
    payload = json.loads(json_path.read_text(encoding="utf-8"))
    bcs = extract_bounds_checks(payload)
    markers = extract_reserved_markers(payload)
    if not bcs or not markers:
        return [], len(bcs), len(markers), len(bcs)

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
                "bc_index": idx,
                "bc": bc,
                "llvm_reserved": marker,
                "llvm_reserved_matched": matched,
                "bc_json_path": str(json_path),
            }
        )
    return rows, len(bcs), len(markers), unmatched


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Download popular crates, run rapx -O, and build BC dataset linked to llvm.reserved markers."
    )
    parser.add_argument("--top-n", type=int, default=10, help="How many popular crates to process.")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("dataset_bc"),
        help="Directory to store crate sources, raw bc json and final dataset.",
    )
    parser.add_argument(
        "--toolchain",
        default="nightly-2025-12-06",
        help="Rust toolchain used for cargo +<toolchain> rapx -O.",
    )
    parser.add_argument(
        "--timeout-sec", type=int, default=1800, help="Timeout (seconds) for each crate analysis."
    )
    args = parser.parse_args()

    output_dir: Path = args.output_dir.resolve()
    sources_dir = output_dir / "sources"
    raw_json_dir = output_dir / "raw_json"
    logs_dir = output_dir / "logs"
    for d in (sources_dir, raw_json_dir, logs_dir):
        d.mkdir(parents=True, exist_ok=True)

    crates = fetch_popular_crates(args.top_n)
    dataset_path = output_dir / "bc_dataset.jsonl"
    manifest_path = output_dir / "manifest.json"

    processed = []
    total_rows = 0
    with dataset_path.open("w", encoding="utf-8") as dataset_f:
        for item in crates:
            crate = item["name"]
            version = item["version"]
            status: Dict[str, Any] = {"crate": crate, "version": version}
            try:
                with tempfile.TemporaryDirectory(prefix=f"{crate}-", dir=str(sources_dir)) as tmp:
                    crate_dir = download_and_extract_crate(crate, version, Path(tmp))
                    ok, rapx_output = run_rapx(crate_dir, args.toolchain, args.timeout_sec)
                    (logs_dir / f"{crate}-{version}.log").write_text(rapx_output, encoding="utf-8")
                    if not ok:
                        status["status"] = "rapx_failed"
                        processed.append(status)
                        continue

                    json_path = find_latest_bc_json(crate_dir)
                    if json_path is None:
                        status["status"] = "bc_json_not_found"
                        processed.append(status)
                        continue

                    copied_json = raw_json_dir / f"{crate}-{version}.json"
                    shutil.copy2(json_path, copied_json)

                    rows, bc_count, marker_count, unmatched = build_dataset_rows(
                        crate, version, copied_json
                    )
                    if not rows:
                        status["status"] = "empty_bc_or_reserved"
                        status["bc_count"] = bc_count
                        status["reserved_count"] = marker_count
                        processed.append(status)
                        continue

                    for row in rows:
                        dataset_f.write(json.dumps(row, ensure_ascii=False) + "\n")

                    total_rows += len(rows)
                    status["status"] = "ok"
                    status["bc_count"] = bc_count
                    status["reserved_count"] = marker_count
                    status["dataset_rows"] = len(rows)
                    status["unmatched_rows"] = unmatched
                    processed.append(status)
            except Exception as exc:
                status["status"] = "error"
                status["error"] = str(exc)
                processed.append(status)

    manifest = {
        "top_n": args.top_n,
        "toolchain": args.toolchain,
        "output_dir": str(output_dir),
        "dataset_path": str(dataset_path),
        "total_rows": total_rows,
        "processed": processed,
    }
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(manifest, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
