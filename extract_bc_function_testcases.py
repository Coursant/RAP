#!/usr/bin/env python3
import argparse
import json
import re
import subprocess
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import collect_popular_crates_bc_dataset as dataset_script


STATUS_OK = "ok"
STATUS_SOURCE_NOT_FOUND = "source_not_found"
STATUS_FUNCTION_NOT_FOUND = "function_not_found"
STATUS_UNSUPPORTED_NESTED_CONTEXT = "unsupported_nested_context"
STATUS_CARGO_CHECK_FAILED = "cargo_check_failed"
STATUS_TIMEOUT = "timeout"


FN_RE = re.compile(r"\bfn\s+[A-Za-z_][A-Za-z0-9_]*")
SLUG_RE = re.compile(r"[^A-Za-z0-9_]+")


def _first_present(d: Dict[str, Any], keys: List[str]) -> Any:
    for key in keys:
        if key in d:
            return d[key]
    return None


def _to_int(value: Any) -> Optional[int]:
    if isinstance(value, int):
        return value
    if isinstance(value, str) and value.isdigit():
        return int(value)
    return None


def summarize_output_tail(text: str, max_lines: int = 20) -> str:
    lines = [line.rstrip() for line in text.splitlines() if line.strip()]
    if not lines:
        return ""
    return "\n".join(lines[-max_lines:])


def stable_slug(value: str, fallback: str = "testcase") -> str:
    slug = SLUG_RE.sub("_", value).strip("_").lower()
    slug = re.sub(r"_+", "_", slug)
    return slug or fallback


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def report_timestamp(generated_at: str) -> str:
    return (
        generated_at.replace("+00:00", "Z")
        .replace(":", "")
        .replace("-", "")
    )


def build_run_report(index: Dict[str, Any]) -> str:
    testcases = index.get("testcases", [])
    status_counts: Dict[str, int] = {}
    for item in testcases:
        status = str(item.get("check_status", "unknown"))
        status_counts[status] = status_counts.get(status, 0) + 1

    lines = [
        "RAPx Bounds-Check Function Testcase Extraction Report",
        f"generated_at: {index.get('generated_at')}",
        "",
        "Configuration",
        f"- dataset_path: {index.get('dataset_path')}",
        f"- sources_dir: {index.get('sources_dir')}",
        f"- output_dir: {index.get('output_dir')}",
        f"- timeout_sec: {index.get('timeout_sec')}",
        f"- only_retained: {index.get('only_retained')}",
        f"- run_report_path: {index.get('run_report_path')}",
        "",
        "Aggregate Metrics",
        f"- total_function_count: {index.get('total_function_count')}",
        f"- ok_count: {index.get('ok_count')}",
        f"- failed_count: {index.get('failed_count')}",
    ]

    if status_counts:
        lines.extend(["", "Status Counts"])
        for status in sorted(status_counts):
            lines.append(f"- {status}: {status_counts[status]}")

    if testcases:
        lines.extend(["", "Per-Testcase Summary"])
        header = (
            "crate".ljust(18)
            + " "
            + "version".ljust(12)
            + " "
            + "status".ljust(28)
            + " "
            + "bc".ljust(6)
            + " "
            + "testcase_dir"
        )
        lines.append(header)
        lines.append("-" * len(header))
        for item in testcases:
            lines.append(
                str(item.get("crate", "")).ljust(18)[:18]
                + " "
                + str(item.get("version", "")).ljust(12)[:12]
                + " "
                + str(item.get("check_status", "")).ljust(28)[:28]
                + " "
                + str(len(item.get("bc_indexes", []))).ljust(6)[:6]
                + " "
                + str(item.get("testcase_dir", ""))
            )

    failures = [item for item in testcases if item.get("check_status") != STATUS_OK]
    if failures:
        lines.extend(["", "Failures Detail"])
        for item in failures:
            lines.append(
                f"- {item.get('crate')}@{item.get('version')} "
                f"function={item.get('function_name')} "
                f"status={item.get('check_status')} "
                f"source_file={item.get('source_file')} "
                f"testcase_dir={item.get('testcase_dir')}"
            )
            tail = item.get("check_output_tail")
            if tail:
                for line in str(tail).splitlines():
                    lines.append(f"    {line}")

    return "\n".join(lines) + "\n"


def record_location(record: Dict[str, Any]) -> Tuple[Optional[str], Optional[int]]:
    bc = record.get("bc") if isinstance(record.get("bc"), dict) else {}
    location = bc.get("location") if isinstance(bc.get("location"), dict) else {}
    file_name = _first_present(location, ["file", "filename", "source_file"])
    line = _to_int(_first_present(location, ["line", "line_no", "source_line"]))
    if file_name is None or line is None:
        file_name = _first_present(bc, ["file", "filename", "source_file"])
        line = _to_int(_first_present(bc, ["line", "line_no", "source_line"]))
    return (str(file_name) if file_name is not None else None, line)


def record_function_name(record: Dict[str, Any]) -> str:
    bc = record.get("bc") if isinstance(record.get("bc"), dict) else {}
    function_context = (
        bc.get("function_context") if isinstance(bc.get("function_context"), dict) else {}
    )
    name = function_context.get("name")
    if not name and isinstance(record.get("llvm_reserved"), dict):
        name = record["llvm_reserved"].get("function")
    return str(name) if name else "<unknown>"


def group_records_by_function(records: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    groups: Dict[Tuple[str, str, str, str, str], Dict[str, Any]] = {}
    order: List[Tuple[str, str, str, str, str]] = []
    for record in records:
        crate = str(record.get("crate", ""))
        version = str(record.get("version", ""))
        file_name, _ = record_location(record)
        function_name = record_function_name(record)
        key = (crate, version, str(record.get("rank")), file_name or "", function_name)
        if key not in groups:
            groups[key] = {
                "crate": crate,
                "version": version,
                "rank": record.get("rank"),
                "source_file": file_name,
                "function_name": function_name,
                "records": [],
            }
            order.append(key)
        groups[key]["records"].append(record)
    return [groups[key] for key in order]


def mask_rust_non_code(text: str) -> str:
    chars = list(text)
    i = 0
    state = "code"
    block_depth = 0
    raw_hashes = 0
    while i < len(chars):
        ch = chars[i]
        nxt = chars[i + 1] if i + 1 < len(chars) else ""
        if state == "code":
            if ch == "/" and nxt == "/":
                chars[i] = chars[i + 1] = " "
                i += 2
                state = "line_comment"
                continue
            if ch == "/" and nxt == "*":
                chars[i] = chars[i + 1] = " "
                i += 2
                state = "block_comment"
                block_depth = 1
                continue
            if ch == "r":
                j = i + 1
                hashes = 0
                while j < len(chars) and chars[j] == "#":
                    hashes += 1
                    j += 1
                if j < len(chars) and chars[j] == '"':
                    chars[i] = " "
                    for k in range(i + 1, j + 1):
                        chars[k] = " "
                    i = j + 1
                    state = "raw_string"
                    raw_hashes = hashes
                    continue
            if ch == '"':
                chars[i] = " "
                i += 1
                state = "string"
                continue
            if ch == "'":
                chars[i] = " "
                i += 1
                state = "char"
                continue
            i += 1
            continue

        if state == "line_comment":
            if ch == "\n":
                state = "code"
            else:
                chars[i] = " "
            i += 1
            continue

        if state == "block_comment":
            if ch == "/" and nxt == "*":
                chars[i] = chars[i + 1] = " "
                block_depth += 1
                i += 2
                continue
            if ch == "*" and nxt == "/":
                chars[i] = chars[i + 1] = " "
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "code"
                continue
            if ch != "\n":
                chars[i] = " "
            i += 1
            continue

        if state == "string":
            if ch == "\\":
                chars[i] = " "
                if i + 1 < len(chars):
                    chars[i + 1] = " "
                i += 2
                continue
            if ch == '"':
                chars[i] = " "
                state = "code"
            elif ch != "\n":
                chars[i] = " "
            i += 1
            continue

        if state == "char":
            if ch == "\\":
                chars[i] = " "
                if i + 1 < len(chars):
                    chars[i + 1] = " "
                i += 2
                continue
            if ch == "'":
                chars[i] = " "
                state = "code"
            elif ch != "\n":
                chars[i] = " "
            i += 1
            continue

        if state == "raw_string":
            if ch == '"' and text[i + 1 : i + 1 + raw_hashes] == ("#" * raw_hashes):
                chars[i] = " "
                for k in range(i + 1, i + 1 + raw_hashes):
                    chars[k] = " "
                i += 1 + raw_hashes
                state = "code"
                continue
            if ch != "\n":
                chars[i] = " "
            i += 1
            continue

    return "".join(chars)


def line_start_offsets(text: str) -> List[int]:
    offsets = [0]
    for idx, ch in enumerate(text):
        if ch == "\n":
            offsets.append(idx + 1)
    return offsets


def line_for_offset(offsets: List[int], offset: int) -> int:
    lo = 0
    hi = len(offsets)
    while lo + 1 < hi:
        mid = (lo + hi) // 2
        if offsets[mid] <= offset:
            lo = mid
        else:
            hi = mid
    return lo + 1


def brace_depth_at(masked: str, offset: int) -> int:
    depth = 0
    for ch in masked[:offset]:
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth = max(0, depth - 1)
    return depth


def find_matching_brace(masked: str, open_offset: int) -> Optional[int]:
    depth = 0
    for idx in range(open_offset, len(masked)):
        ch = masked[idx]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return idx
    return None


def include_leading_attrs_and_docs(lines: List[str], fn_start_line: int) -> int:
    start = fn_start_line
    i = fn_start_line - 2
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("#[") or stripped.startswith("///") or stripped.startswith("//!"):
            start = i + 1
            i -= 1
            continue
        break
    return start


def extract_function_at_line(source_text: str, target_line: int) -> Tuple[Optional[Dict[str, Any]], Optional[str]]:
    masked = mask_rust_non_code(source_text)
    offsets = line_start_offsets(source_text)
    lines = source_text.splitlines()

    candidates: List[Dict[str, Any]] = []
    for match in FN_RE.finditer(masked):
        open_offset = masked.find("{", match.end())
        if open_offset == -1:
            continue
        close_offset = find_matching_brace(masked, open_offset)
        if close_offset is None:
            continue
        fn_start_line = line_for_offset(offsets, match.start())
        fn_end_line = line_for_offset(offsets, close_offset)
        if fn_start_line <= target_line <= fn_end_line:
            item_start_line = include_leading_attrs_and_docs(lines, fn_start_line)
            candidates.append(
                {
                    "fn_start_line": fn_start_line,
                    "start_line": item_start_line,
                    "end_line": fn_end_line,
                    "start_offset": offsets[item_start_line - 1],
                    "end_offset": close_offset + 1,
                    "brace_depth": brace_depth_at(masked, match.start()),
                }
            )

    if not candidates:
        return None, STATUS_FUNCTION_NOT_FOUND

    selected = max(candidates, key=lambda c: c["fn_start_line"])
    if selected["brace_depth"] != 0:
        return selected, STATUS_UNSUPPORTED_NESTED_CONTEXT

    selected["source"] = source_text[selected["start_offset"] : selected["end_offset"]]
    return selected, None


def read_cargo_payload(crate_dir: Path) -> Tuple[Dict[str, Any], str]:
    cargo_toml = crate_dir / "Cargo.toml"
    text = cargo_toml.read_text(encoding="utf-8")
    return dataset_script._load_toml_payload(text), text


def cargo_edition(crate_dir: Path) -> str:
    try:
        payload, _ = read_cargo_payload(crate_dir)
        edition = payload.get("package", {}).get("edition")
        return str(edition) if edition else "2021"
    except Exception:
        return "2021"


def extract_dependency_sections(cargo_text: str) -> str:
    wanted = {"dependencies", "dev-dependencies", "build-dependencies"}
    lines = cargo_text.splitlines()
    sections: List[str] = []
    current: List[str] = []
    include = False

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            if include and current:
                sections.extend(current)
                sections.append("")
            section_name = stripped.strip("[]").strip()
            include = section_name in wanted
            current = [line] if include else []
            continue
        if include:
            current.append(line)

    if include and current:
        sections.extend(current)

    while sections and sections[-1] == "":
        sections.pop()
    return "\n".join(sections)


def render_cargo_toml(crate_dir: Path, package_name: str) -> str:
    edition = cargo_edition(crate_dir)
    cargo_text = (crate_dir / "Cargo.toml").read_text(encoding="utf-8")
    dependency_sections = extract_dependency_sections(cargo_text)
    rendered = [
        "[package]",
        f'name = "{package_name}"',
        'version = "0.1.0"',
        f'edition = "{edition}"',
        "",
    ]
    if dependency_sections:
        rendered.append(dependency_sections)
        rendered.append("")
    return "\n".join(rendered)


def render_lib_rs(function_source: str) -> str:
    return "\n".join(
        [
            "#![allow(dead_code)]",
            "#![allow(unused_imports)]",
            "#![allow(unused_variables)]",
            "#![allow(unused_mut)]",
            "#![allow(non_snake_case)]",
            "",
            function_source.rstrip(),
            "",
        ]
    )


def run_cargo_check(crate_dir: Path, timeout_sec: int) -> Tuple[str, str, float]:
    started_at = time.monotonic()
    try:
        proc = subprocess.run(
            ["cargo", "check"],
            cwd=crate_dir,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout_sec,
            check=False,
        )
        elapsed = time.monotonic() - started_at
    except subprocess.TimeoutExpired as exc:
        output = exc.output or ""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        return STATUS_TIMEOUT, summarize_output_tail(str(output)), time.monotonic() - started_at

    status = STATUS_OK if proc.returncode == 0 else STATUS_CARGO_CHECK_FAILED
    return status, summarize_output_tail(proc.stdout), elapsed


def testcase_metadata(
    *,
    group: Dict[str, Any],
    crate_dir: Path,
    source_path: Path,
    extracted: Dict[str, Any],
    check_status: str,
    check_output_tail: str,
    check_elapsed_sec: float,
) -> Dict[str, Any]:
    records = group["records"]
    source_lines = []
    bc_indexes = []
    retained_values = []
    matched_values = []
    for record in records:
        _, line = record_location(record)
        source_lines.append(line)
        bc_indexes.append(record.get("bc_index"))
        retained_values.append(record.get("llvm_retained"))
        matched_values.append(record.get("llvm_reserved_matched"))

    return {
        "crate": group["crate"],
        "version": group["version"],
        "rank": group.get("rank"),
        "function_name": group["function_name"],
        "source_dir": str(crate_dir),
        "source_file": str(source_path),
        "source_lines": source_lines,
        "extract_start_line": extracted["start_line"],
        "extract_end_line": extracted["end_line"],
        "bc_indexes": bc_indexes,
        "llvm_retained_values": retained_values,
        "llvm_reserved_matched_values": matched_values,
        "record_count": len(records),
        "records": records,
        "check_status": check_status,
        "check_output_tail": check_output_tail,
        "check_elapsed_sec": round(check_elapsed_sec, 3),
    }


def process_group(
    group: Dict[str, Any],
    *,
    sources_dir: Path,
    output_dir: Path,
    timeout_sec: int,
) -> Dict[str, Any]:
    crate = group["crate"]
    version = group["version"]
    source_file = group.get("source_file")
    function_name = group["function_name"]
    slug = stable_slug(function_name, "function")
    testcase_dir = output_dir / f"{crate}-{version}" / slug

    result = {
        "crate": crate,
        "version": version,
        "rank": group.get("rank"),
        "function_name": function_name,
        "source_file": source_file,
        "bc_indexes": [record.get("bc_index") for record in group["records"]],
        "testcase_dir": str(testcase_dir),
        "check_status": "",
        "check_output_tail": "",
    }

    try:
        crate_dir = dataset_script.resolve_existing_staged_crate_source(
            crate, version, sources_dir
        )
    except Exception as exc:
        result["check_status"] = STATUS_SOURCE_NOT_FOUND
        result["check_output_tail"] = str(exc)
        return result

    if not source_file:
        result["check_status"] = STATUS_SOURCE_NOT_FOUND
        result["check_output_tail"] = "record has no source file"
        return result

    source_path = (crate_dir / source_file).resolve()
    try:
        source_path.relative_to(crate_dir.resolve())
    except ValueError:
        result["check_status"] = STATUS_SOURCE_NOT_FOUND
        result["check_output_tail"] = f"source file escapes crate dir: {source_file}"
        return result

    if not source_path.exists():
        result["check_status"] = STATUS_SOURCE_NOT_FOUND
        result["check_output_tail"] = f"source file not found: {source_path}"
        return result

    lines = [record_location(record)[1] for record in group["records"]]
    target_line = min(line for line in lines if line is not None) if any(lines) else None
    if target_line is None:
        result["check_status"] = STATUS_FUNCTION_NOT_FOUND
        result["check_output_tail"] = "records have no source lines"
        return result

    source_text = source_path.read_text(encoding="utf-8")
    extracted, error_status = extract_function_at_line(source_text, target_line)
    if error_status is not None:
        result["check_status"] = error_status
        result["check_output_tail"] = (
            f"function candidate is nested at line {extracted['fn_start_line']}"
            if extracted and error_status == STATUS_UNSUPPORTED_NESTED_CONTEXT
            else f"no function contains line {target_line}"
        )
        return result
    assert extracted is not None

    package_name = stable_slug(f"{crate}_{version}_{slug}", "bc_testcase")
    src_dir = testcase_dir / "src"
    src_dir.mkdir(parents=True, exist_ok=True)
    (testcase_dir / "Cargo.toml").write_text(
        render_cargo_toml(crate_dir, package_name), encoding="utf-8"
    )
    (src_dir / "lib.rs").write_text(render_lib_rs(extracted["source"]), encoding="utf-8")

    check_status, check_output_tail, check_elapsed_sec = run_cargo_check(
        testcase_dir, timeout_sec
    )
    metadata = testcase_metadata(
        group=group,
        crate_dir=crate_dir,
        source_path=source_path,
        extracted=extracted,
        check_status=check_status,
        check_output_tail=check_output_tail,
        check_elapsed_sec=check_elapsed_sec,
    )
    write_json(testcase_dir / "bc_metadata.json", metadata)

    result.update(
        {
            "source_dir": str(crate_dir),
            "source_file": str(source_path),
            "extract_start_line": extracted["start_line"],
            "extract_end_line": extracted["end_line"],
            "record_count": len(group["records"]),
            "check_status": check_status,
            "check_output_tail": check_output_tail,
            "check_elapsed_sec": round(check_elapsed_sec, 3),
            "metadata_path": str(testcase_dir / "bc_metadata.json"),
        }
    )
    return result


def build_testcases(
    *,
    dataset_path: Path,
    sources_dir: Path,
    output_dir: Path,
    timeout_sec: int,
    only_retained: bool = False,
) -> Dict[str, Any]:
    dataset = load_json(dataset_path)
    records = dataset.get("records", []) if isinstance(dataset, dict) else []
    records = [record for record in records if isinstance(record, dict)]
    if only_retained:
        records = [record for record in records if record.get("llvm_retained") is True]

    groups = group_records_by_function(records)
    output_dir.mkdir(parents=True, exist_ok=True)
    run_reports_dir = output_dir / "run_reports"
    run_reports_dir.mkdir(parents=True, exist_ok=True)
    results = [
        process_group(
            group,
            sources_dir=sources_dir,
            output_dir=output_dir,
            timeout_sec=timeout_sec,
        )
        for group in groups
    ]

    ok_count = sum(1 for result in results if result.get("check_status") == STATUS_OK)
    failed_count = len(results) - ok_count
    generated_at = dataset_script.utc_now_iso()
    run_report_path = (
        run_reports_dir / f"extract_report_{report_timestamp(generated_at)}.log"
    )
    index = {
        "generated_at": generated_at,
        "dataset_path": str(dataset_path),
        "sources_dir": str(sources_dir),
        "output_dir": str(output_dir),
        "run_report_path": str(run_report_path),
        "timeout_sec": timeout_sec,
        "only_retained": only_retained,
        "total_function_count": len(results),
        "ok_count": ok_count,
        "failed_count": failed_count,
        "testcases": results,
    }
    run_report_path.write_text(build_run_report(index), encoding="utf-8")
    write_json(output_dir / "function_testcases_index.json", index)
    return index


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Extract standalone Cargo testcase crates for functions containing bounds checks."
    )
    parser.add_argument(
        "--dataset-path",
        type=Path,
        default=Path("dataset_bc/bounds_checks_dataset.json"),
        help="Path to bounds_checks_dataset.json.",
    )
    parser.add_argument(
        "--sources-dir",
        type=Path,
        default=Path("dataset_bc/sources"),
        help="Directory containing staged crate sources.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("dataset_bc/function_testcases"),
        help="Directory where testcase crates and index JSON are written.",
    )
    parser.add_argument(
        "--timeout-sec",
        type=int,
        default=120,
        help="Timeout for each generated testcase cargo check.",
    )
    parser.add_argument(
        "--only-retained",
        action="store_true",
        help="Only extract records whose llvm_retained value is true.",
    )
    args = parser.parse_args()

    index = build_testcases(
        dataset_path=args.dataset_path.resolve(),
        sources_dir=args.sources_dir.resolve(),
        output_dir=args.output_dir.resolve(),
        timeout_sec=args.timeout_sec,
        only_retained=args.only_retained,
    )
    print(json.dumps(index, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
