import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import extract_bc_function_testcases as script


def make_record(
    *,
    crate: str = "demo",
    version: str = "0.1.0",
    bc_index: int = 0,
    line: int = 6,
    function_name: str = "demo::checked",
    retained: bool = True,
) -> dict:
    return {
        "crate": crate,
        "version": version,
        "rank": 1,
        "bc_index": bc_index,
        "bc": {
            "location": {"file": "src/lib.rs", "line": line},
            "function_context": {"name": function_name},
            "symbolic_features": {},
            "call_context": {},
        },
        "llvm_reserved": {
            "file": "src/lib.rs",
            "line": line,
            "function": function_name,
            "retained": retained,
        },
        "llvm_reserved_matched": True,
        "llvm_retained": retained,
        "raw_json_path": "/tmp/raw.json",
    }


def write_demo_crate(crate_dir: Path, source: str) -> None:
    (crate_dir / "src").mkdir(parents=True, exist_ok=True)
    (crate_dir / "Cargo.toml").write_text(
        "\n".join(
            [
                "[package]",
                "name = 'demo'",
                "version = '0.1.0'",
                "edition = '2021'",
                "",
                "[dependencies]",
                "itoa = '1'",
                "",
            ]
        ),
        encoding="utf-8",
    )
    (crate_dir / "src" / "lib.rs").write_text(source, encoding="utf-8")


class ExtractBcFunctionTestcasesTests(unittest.TestCase):
    def test_group_records_by_function_merges_same_function(self) -> None:
        records = [
            make_record(bc_index=0, line=5, function_name="demo::checked"),
            make_record(bc_index=1, line=6, function_name="demo::checked"),
            make_record(bc_index=2, line=12, function_name="demo::other"),
        ]

        groups = script.group_records_by_function(records)

        self.assertEqual(len(groups), 2)
        self.assertEqual(groups[0]["function_name"], "demo::checked")
        self.assertEqual([r["bc_index"] for r in groups[0]["records"]], [0, 1])
        self.assertEqual(groups[1]["function_name"], "demo::other")

    def test_extract_function_at_line_includes_attrs_and_docs(self) -> None:
        source = "\n".join(
            [
                "use std::cmp;",
                "/// doc",
                "#[inline]",
                "pub fn checked(slice: &[i32], idx: usize) -> i32 {",
                "    let i = cmp::min(idx, slice.len() - 1);",
                "    slice[i]",
                "}",
                "",
                "pub fn other() {}",
                "",
            ]
        )

        extracted, error = script.extract_function_at_line(source, 6)

        self.assertIsNone(error)
        self.assertIsNotNone(extracted)
        assert extracted is not None
        self.assertEqual(extracted["start_line"], 2)
        self.assertEqual(extracted["end_line"], 7)
        self.assertIn("/// doc", extracted["source"])
        self.assertIn("#[inline]", extracted["source"])
        self.assertIn("pub fn checked", extracted["source"])
        self.assertNotIn("pub fn other", extracted["source"])

    def test_extract_function_at_line_rejects_nested_method(self) -> None:
        source = "\n".join(
            [
                "pub struct Demo;",
                "impl Demo {",
                "    pub fn checked(&self, slice: &[i32], idx: usize) -> i32 {",
                "        slice[idx]",
                "    }",
                "}",
                "",
            ]
        )

        extracted, error = script.extract_function_at_line(source, 4)

        self.assertEqual(error, script.STATUS_UNSUPPORTED_NESTED_CONTEXT)
        self.assertIsNotNone(extracted)

    def test_build_testcases_writes_checked_crate_and_index(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            dataset_path = root / "bounds_checks_dataset.json"
            sources_dir = root / "sources"
            crate_dir = sources_dir / "demo-0.1.0"
            output_dir = root / "function_testcases"
            source = "\n".join(
                [
                    "/// top-level function",
                    "#[inline]",
                    "pub fn checked(slice: &[i32], idx: usize) -> i32 {",
                    "    slice[idx]",
                    "}",
                    "",
                ]
            )
            write_demo_crate(crate_dir, source)
            dataset_path.write_text(
                json.dumps(
                    {
                        "metadata": {},
                        "records": [
                            make_record(bc_index=0, line=4),
                            make_record(bc_index=1, line=4),
                        ],
                    }
                ),
                encoding="utf-8",
            )

            completed = subprocess.CompletedProcess(
                ["cargo", "check"], 0, stdout="checking demo\nfinished\n"
            )
            with mock.patch.object(script.subprocess, "run", return_value=completed) as run:
                index = script.build_testcases(
                    dataset_path=dataset_path,
                    sources_dir=sources_dir,
                    output_dir=output_dir,
                    timeout_sec=9,
                )

            self.assertEqual(index["total_function_count"], 1)
            self.assertEqual(index["ok_count"], 1)
            self.assertEqual(index["failed_count"], 0)
            report_path = Path(index["run_report_path"])
            self.assertTrue(report_path.exists())
            self.assertEqual(report_path.parent, output_dir / "run_reports")
            self.assertIn("Aggregate Metrics", report_path.read_text(encoding="utf-8"))
            testcase = index["testcases"][0]
            self.assertEqual(testcase["check_status"], script.STATUS_OK)
            testcase_dir = Path(testcase["testcase_dir"])
            self.assertTrue((testcase_dir / "Cargo.toml").exists())
            self.assertTrue((testcase_dir / "src" / "lib.rs").exists())
            self.assertTrue((testcase_dir / "bc_metadata.json").exists())
            self.assertTrue((output_dir / "function_testcases_index.json").exists())
            self.assertIn("pub fn checked", (testcase_dir / "src" / "lib.rs").read_text())
            metadata = json.loads((testcase_dir / "bc_metadata.json").read_text())
            self.assertEqual(metadata["bc_indexes"], [0, 1])
            self.assertEqual(metadata["check_status"], script.STATUS_OK)
            run.assert_called_once()
            self.assertEqual(run.call_args.kwargs["cwd"], testcase_dir)
            self.assertEqual(run.call_args.kwargs["timeout"], 9)

    def test_build_testcases_records_cargo_check_failure(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            dataset_path = root / "bounds_checks_dataset.json"
            sources_dir = root / "sources"
            crate_dir = sources_dir / "demo-0.1.0"
            output_dir = root / "function_testcases"
            write_demo_crate(
                crate_dir,
                "pub fn checked(slice: &[i32], idx: usize) -> i32 {\n    slice[idx]\n}\n",
            )
            dataset_path.write_text(
                json.dumps({"records": [make_record(bc_index=0, line=2)]}),
                encoding="utf-8",
            )
            completed = subprocess.CompletedProcess(
                ["cargo", "check"], 101, stdout="error line 1\nerror line 2\n"
            )

            with mock.patch.object(script.subprocess, "run", return_value=completed):
                index = script.build_testcases(
                    dataset_path=dataset_path,
                    sources_dir=sources_dir,
                    output_dir=output_dir,
                    timeout_sec=120,
                )

            self.assertEqual(index["ok_count"], 0)
            self.assertEqual(index["failed_count"], 1)
            self.assertEqual(
                index["testcases"][0]["check_status"], script.STATUS_CARGO_CHECK_FAILED
            )
            self.assertIn("error line 2", index["testcases"][0]["check_output_tail"])
            report = Path(index["run_report_path"]).read_text(encoding="utf-8")
            self.assertIn("Failures Detail", report)
            self.assertIn("cargo_check_failed", report)
            self.assertIn("error line 2", report)

    def test_build_testcases_records_timeout(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            dataset_path = root / "bounds_checks_dataset.json"
            sources_dir = root / "sources"
            crate_dir = sources_dir / "demo-0.1.0"
            output_dir = root / "function_testcases"
            write_demo_crate(
                crate_dir,
                "pub fn checked(slice: &[i32], idx: usize) -> i32 {\n    slice[idx]\n}\n",
            )
            dataset_path.write_text(
                json.dumps({"records": [make_record(bc_index=0, line=2)]}),
                encoding="utf-8",
            )

            with mock.patch.object(
                script.subprocess,
                "run",
                side_effect=subprocess.TimeoutExpired(
                    ["cargo", "check"], timeout=1, output="partial output\n"
                ),
            ):
                index = script.build_testcases(
                    dataset_path=dataset_path,
                    sources_dir=sources_dir,
                    output_dir=output_dir,
                    timeout_sec=1,
                )

            self.assertEqual(index["testcases"][0]["check_status"], script.STATUS_TIMEOUT)
            self.assertIn("partial output", index["testcases"][0]["check_output_tail"])

    def test_build_testcases_records_source_and_function_failures(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            sources_dir = root / "sources"
            output_dir = root / "function_testcases"
            dataset_path = root / "bounds_checks_dataset.json"
            crate_dir = sources_dir / "demo-0.1.0"
            write_demo_crate(crate_dir, "pub fn other() {}\n")
            dataset_path.write_text(
                json.dumps(
                    {
                        "records": [
                            make_record(crate="missing", version="0.1.0", bc_index=0, line=1),
                            make_record(bc_index=1, line=99, function_name="demo::checked"),
                        ]
                    }
                ),
                encoding="utf-8",
            )

            index = script.build_testcases(
                dataset_path=dataset_path,
                sources_dir=sources_dir,
                output_dir=output_dir,
                timeout_sec=120,
            )

            statuses = [item["check_status"] for item in index["testcases"]]
            self.assertIn(script.STATUS_SOURCE_NOT_FOUND, statuses)
            self.assertIn(script.STATUS_FUNCTION_NOT_FOUND, statuses)

    def test_build_testcases_only_retained_filters_records(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            dataset_path = root / "bounds_checks_dataset.json"
            sources_dir = root / "sources"
            crate_dir = sources_dir / "demo-0.1.0"
            output_dir = root / "function_testcases"
            write_demo_crate(
                crate_dir,
                "\n".join(
                    [
                        "pub fn kept(slice: &[i32], idx: usize) -> i32 {",
                        "    slice[idx]",
                        "}",
                        "pub fn dropped(slice: &[i32], idx: usize) -> i32 {",
                        "    slice[idx]",
                        "}",
                        "",
                    ]
                ),
            )
            dataset_path.write_text(
                json.dumps(
                    {
                        "records": [
                            make_record(
                                bc_index=0,
                                line=2,
                                function_name="demo::kept",
                                retained=True,
                            ),
                            make_record(
                                bc_index=1,
                                line=5,
                                function_name="demo::dropped",
                                retained=False,
                            ),
                        ]
                    }
                ),
                encoding="utf-8",
            )
            completed = subprocess.CompletedProcess(["cargo", "check"], 0, stdout="ok\n")

            with mock.patch.object(script.subprocess, "run", return_value=completed):
                index = script.build_testcases(
                    dataset_path=dataset_path,
                    sources_dir=sources_dir,
                    output_dir=output_dir,
                    timeout_sec=120,
                    only_retained=True,
                )

            self.assertEqual(index["total_function_count"], 1)
            self.assertEqual(index["testcases"][0]["function_name"], "demo::kept")

    def test_main_prints_index(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            dataset_path = root / "bounds_checks_dataset.json"
            sources_dir = root / "sources"
            output_dir = root / "out"
            dataset_path.write_text(json.dumps({"records": []}), encoding="utf-8")
            argv = [
                "extract_bc_function_testcases.py",
                "--dataset-path",
                str(dataset_path),
                "--sources-dir",
                str(sources_dir),
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(sys, "argv", argv):
                script.main()

            self.assertTrue((output_dir / "function_testcases_index.json").exists())
            index = json.loads(
                (output_dir / "function_testcases_index.json").read_text(encoding="utf-8")
            )
            self.assertTrue(Path(index["run_report_path"]).exists())

    def test_build_testcases_writes_distinct_run_reports_for_repeated_runs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            dataset_path = root / "bounds_checks_dataset.json"
            sources_dir = root / "sources"
            output_dir = root / "function_testcases"
            dataset_path.write_text(json.dumps({"records": []}), encoding="utf-8")

            with mock.patch.object(
                script.dataset_script,
                "utc_now_iso",
                return_value="2026-01-01T00:00:00+00:00",
            ):
                first = script.build_testcases(
                    dataset_path=dataset_path,
                    sources_dir=sources_dir,
                    output_dir=output_dir,
                    timeout_sec=120,
                )
            with mock.patch.object(
                script.dataset_script,
                "utc_now_iso",
                return_value="2026-01-01T00:00:01+00:00",
            ):
                second = script.build_testcases(
                    dataset_path=dataset_path,
                    sources_dir=sources_dir,
                    output_dir=output_dir,
                    timeout_sec=120,
                )

            first_report = Path(first["run_report_path"])
            second_report = Path(second["run_report_path"])
            self.assertTrue(first_report.exists())
            self.assertTrue(second_report.exists())
            self.assertNotEqual(first_report, second_report)
            self.assertEqual(
                len(list((output_dir / "run_reports").glob("extract_report_*.log"))),
                2,
            )


if __name__ == "__main__":
    unittest.main()
