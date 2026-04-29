import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

import collect_popular_crates_bc_dataset as script


class CollectPopularCratesDatasetTests(unittest.TestCase):
    def test_fetch_popular_crates_paginates(self) -> None:
        payload_page1 = {
            "crates": [
                {
                    "id": f"crate-{i}",
                    "max_stable_version": f"1.0.{i}",
                    "downloads": 1000 - i,
                }
                for i in range(1, 101)
            ]
        }
        payload_page2 = {
            "crates": [
                {
                    "id": f"crate-{i}",
                    "max_stable_version": f"1.0.{i}",
                    "downloads": 1000 - i,
                }
                for i in range(101, 151)
            ]
        }

        responses = [
            mock.MagicMock(
                __enter__=mock.Mock(return_value=mock.Mock(read=mock.Mock(return_value=json.dumps(payload_page1).encode("utf-8")))),
                __exit__=mock.Mock(return_value=False),
            ),
            mock.MagicMock(
                __enter__=mock.Mock(return_value=mock.Mock(read=mock.Mock(return_value=json.dumps(payload_page2).encode("utf-8")))),
                __exit__=mock.Mock(return_value=False),
            ),
        ]
        opened_urls = []

        def fake_urlopen(req, timeout=30):
            opened_urls.append(req.full_url)
            return responses.pop(0)

        with mock.patch.object(script.urllib.request, "urlopen", side_effect=fake_urlopen):
            crates = script.fetch_popular_crates(150)

        self.assertEqual(len(crates), 150)
        self.assertEqual(crates[0]["rank"], 1)
        self.assertEqual(crates[-1]["rank"], 150)
        self.assertIn("page=1", opened_urls[0])
        self.assertIn("page=2", opened_urls[1])
        self.assertIn("per_page=100", opened_urls[0])
        self.assertIn("per_page=50", opened_urls[1])

    def test_main_writes_dataset_index_when_fetch_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            output_dir = Path(tmp_dir) / "dataset_bc"
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--top-n",
                "3",
                "--output-dir",
                str(output_dir),
                "--toolchain",
                "nightly-2025-12-06",
            ]
            buf = io.StringIO()
            with mock.patch.object(
                script,
                "fetch_popular_crates",
                side_effect=OSError("Network is unreachable"),
            ):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(buf):
                        script.main()

            dataset_index_path = output_dir / "dataset_index.json"
            self.assertTrue(dataset_index_path.exists())
            dataset_index = json.loads(dataset_index_path.read_text(encoding="utf-8"))
            run_report_path = Path(dataset_index["run_report_path"])
            self.assertTrue(run_report_path.exists())
            self.assertEqual(run_report_path.parent, output_dir / "run_reports")
            self.assertNotEqual(run_report_path, output_dir / "run_report.log")
            run_report = run_report_path.read_text(encoding="utf-8")
            self.assertEqual(dataset_index["status"], "fetch_popular_crates_failed")
            self.assertIn("Network is unreachable", dataset_index["error"])
            self.assertEqual(dataset_index["run_report_path"], str(run_report_path))
            self.assertEqual(dataset_index["crates"], [])
            self.assertEqual(dataset_index["total_bc_count"], 0)
            self.assertIn("overall_status: fetch_popular_crates_failed", run_report)
            self.assertIn("Error", run_report)
            self.assertIn("matched_ratio: 0/0", run_report)

    def test_run_rapx_fallbacks_include_stable_and_workspace_env(self) -> None:
        calls = []

        def fake_run(cmd, **kwargs):
            calls.append((cmd, kwargs["env"]["RAP_RECURSIVE"]))
            if cmd[:2] == ["cargo", "+stable"]:
                return subprocess.CompletedProcess(cmd, 0, stdout="ok-stable")
            return subprocess.CompletedProcess(cmd, 1, stdout="failed")

        with tempfile.TemporaryDirectory() as tmp_dir:
            crate_dir = Path(tmp_dir)
            (crate_dir / "Cargo.toml").write_text(
                "[workspace]\nmembers = [\"member-a\"]\n", encoding="utf-8"
            )
            with mock.patch.object(script, "_detect_crate_toolchain", return_value=None):
                with mock.patch.object(script.subprocess, "run", side_effect=fake_run):
                    (
                        ok,
                        output,
                        used_toolchain,
                        elapsed_sec,
                        rap_recursive,
                        last_attempt_command,
                        log_tail,
                    ) = script.run_rapx(
                        crate_dir, "1.71.0", timeout_sec=30
                    )

        self.assertTrue(ok)
        self.assertEqual(used_toolchain, "stable")
        self.assertEqual(rap_recursive, "shallow")
        self.assertGreaterEqual(elapsed_sec, 0.0)
        self.assertEqual(last_attempt_command, "cargo +stable rapx -bounds-db")
        self.assertEqual(log_tail, "ok-stable")
        self.assertIn("$ cargo +1.71.0 rapx -bounds-db", output)
        self.assertIn("$ cargo +stable rapx -bounds-db", output)
        self.assertEqual(calls[0][0], ["cargo", "+1.71.0", "rapx", "-bounds-db"])
        self.assertEqual(calls[0][1], "shallow")
        self.assertEqual(calls[1][0], ["cargo", "+stable", "rapx", "-bounds-db"])

    def test_extract_reserved_markers_reads_llvm_reserved_records(self) -> None:
        payload = {
            "llvm": {
                "reserved": {
                    "analysis": "llvm-ir-release",
                    "records": [{"file": "src/lib.rs", "line": 12, "retained": False}],
                }
            }
        }

        markers = script.extract_reserved_markers(payload)
        self.assertEqual(len(markers), 1)
        self.assertEqual(markers[0]["line"], 12)

    def test_build_dataset_rows_matches_by_nested_location_and_retained(self) -> None:
        payload = {
            "bounds_checks": [
                {
                    "location": {"file": "src/lib.rs", "line": 12},
                    "symbolic_features": {},
                    "function_context": {},
                    "call_context": {},
                },
                {
                    "location": {"file": "src/lib.rs", "line": 99},
                    "symbolic_features": {},
                    "function_context": {},
                    "call_context": {},
                },
            ],
            "llvm": {
                "reserved": {
                    "analysis": "llvm-ir-release",
                    "records": [
                        {
                            "file": "src/lib.rs",
                            "line": 12,
                            "function": "crate::f",
                            "retained": False,
                        }
                    ],
                }
            },
        }

        with tempfile.TemporaryDirectory() as tmp_dir:
            json_path = Path(tmp_dir) / "raw.json"
            json_path.write_text(json.dumps(payload), encoding="utf-8")
            rows, bc_count, marker_count, unmatched = script.build_dataset_rows(
                "syn", "2.0.117", 7, json_path
            )

        self.assertEqual(bc_count, 2)
        self.assertEqual(marker_count, 1)
        self.assertEqual(unmatched, 1)
        self.assertEqual(rows[0]["rank"], 7)
        self.assertTrue(rows[0]["llvm_reserved_matched"])
        self.assertFalse(rows[0]["llvm_retained"])
        self.assertIsNone(rows[1]["llvm_reserved"])
        self.assertIsNone(rows[1]["llvm_retained"])

    def test_main_writes_layered_json_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            output_dir = Path(tmp_dir) / "dataset_bc"
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--top-n",
                "2",
                "--output-dir",
                str(output_dir),
                "--toolchain",
                "nightly-2025-12-06",
            ]

            crates = [
                {
                    "rank": 1,
                    "name": "syn",
                    "version": "2.0.117",
                    "downloads": 100,
                    "fetched_at": "2026-01-01T00:00:00+00:00",
                },
                {
                    "rank": 2,
                    "name": "quote",
                    "version": "1.0.39",
                    "downloads": 90,
                    "fetched_at": "2026-01-01T00:00:00+00:00",
                },
            ]

            def fake_download(crate: str, version: str, dst_dir: Path) -> Path:
                saved = dst_dir / f"{crate}-{version}"
                saved.mkdir(parents=True, exist_ok=True)
                (saved / "Cargo.toml").write_text(
                    "[package]\nname='x'\nversion='0.1.0'\n", encoding="utf-8"
                )
                if crate == "syn":
                    payload = {
                        "bounds_checks": [
                            {
                                "location": {"file": "src/lib.rs", "line": 12},
                                "symbolic_features": {},
                                "function_context": {},
                                "call_context": {},
                            }
                        ],
                        "llvm": {
                            "reserved": {
                                "analysis": "llvm-ir-release",
                                "records": [
                                    {
                                        "file": "src/lib.rs",
                                        "line": 12,
                                        "function": "crate::f",
                                        "retained": True,
                                    }
                                ],
                            }
                        },
                    }
                    (saved / "bounds_checks_syn.json").write_text(
                        json.dumps(payload), encoding="utf-8"
                    )
                return saved

            def fake_run_rapx(crate_dir: Path, toolchain: str, timeout_sec: int):
                if crate_dir.name.startswith("quote-"):
                    return (
                        False,
                        "$ cargo +stable rapx -bounds-db\n# RAP_RECURSIVE=none\nerror line 1\nerror line 2\n",
                        "none",
                        0.123,
                        "none",
                        "cargo +stable rapx -bounds-db",
                        "error line 1\nerror line 2",
                    )
                return (
                    True,
                    "rapx ok",
                    "nightly-2025-12-06",
                    0.456,
                    "none",
                    "cargo +nightly-2025-12-06 rapx -bounds-db",
                    "rapx ok",
                )

            with mock.patch.object(script, "fetch_popular_crates", return_value=crates):
                with mock.patch.object(
                    script, "download_and_extract_crate", side_effect=fake_download
                ):
                    with mock.patch.object(script, "run_rapx", side_effect=fake_run_rapx):
                        with mock.patch.object(sys, "argv", argv):
                            script.main()

            snapshot_path = output_dir / "popular_crates_top2.json"
            dataset_path = output_dir / "bounds_checks_dataset.json"
            dataset_index_path = output_dir / "dataset_index.json"
            syn_status_path = output_dir / "crate_status" / "syn-2.0.117.json"
            quote_status_path = output_dir / "crate_status" / "quote-1.0.39.json"
            syn_raw_json = output_dir / "raw_json" / "syn-2.0.117.json"

            self.assertTrue(snapshot_path.exists())
            self.assertTrue(dataset_path.exists())
            self.assertTrue(dataset_index_path.exists())
            self.assertTrue(syn_status_path.exists())
            self.assertTrue(quote_status_path.exists())
            self.assertTrue(syn_raw_json.exists())

            dataset = json.loads(dataset_path.read_text(encoding="utf-8"))
            dataset_index = json.loads(dataset_index_path.read_text(encoding="utf-8"))
            run_report_path = Path(dataset_index["run_report_path"])
            self.assertTrue(run_report_path.exists())
            self.assertEqual(run_report_path.parent, output_dir / "run_reports")
            self.assertNotEqual(run_report_path, output_dir / "run_report.log")
            run_report = run_report_path.read_text(encoding="utf-8")
            syn_status = json.loads(syn_status_path.read_text(encoding="utf-8"))
            quote_status = json.loads(quote_status_path.read_text(encoding="utf-8"))

            self.assertEqual(dataset["metadata"]["crate_count"], 2)
            self.assertEqual(dataset["metadata"]["success_count"], 1)
            self.assertEqual(dataset["metadata"]["failed_count"], 1)
            self.assertEqual(dataset["metadata"]["total_bc_count"], 1)
            self.assertEqual(dataset["metadata"]["matched_bc_count"], 1)
            self.assertEqual(dataset["metadata"]["retained_bc_count"], 1)
            self.assertEqual(dataset["metadata"]["run_report_path"], str(run_report_path))
            self.assertEqual(len(dataset["records"]), 1)
            self.assertEqual(dataset["records"][0]["crate"], "syn")
            self.assertTrue(dataset["records"][0]["llvm_reserved_matched"])
            self.assertTrue(dataset["records"][0]["llvm_retained"])

            self.assertEqual(dataset_index["status"], "ok")
            self.assertEqual(dataset_index["run_report_path"], str(run_report_path))
            self.assertEqual(dataset_index["success_count"], 1)
            self.assertEqual(dataset_index["failed_count"], 1)
            self.assertEqual(dataset_index["total_bc_count"], 1)
            self.assertEqual(len(dataset_index["crates"]), 2)
            self.assertIn("Aggregate Metrics", run_report)
            self.assertIn("- success_count: 1", run_report)
            self.assertIn("- failed_count: 1", run_report)
            self.assertIn("- matched_ratio: 1/1", run_report)
            self.assertIn("crate              version", run_report)
            self.assertIn("toolchain", run_report)
            self.assertIn("retained", run_report)
            self.assertIn("syn                2.0.117", run_report)
            self.assertIn("quote              1.0.39", run_report)
            self.assertIn("Failures Detail", run_report)
            self.assertIn("quote@1.0.39 status=rapx_failed", run_report)
            self.assertIn("last_cmd=cargo +stable rapx -bounds-db", run_report)
            self.assertIn("error line 1", run_report)
            self.assertIn("error line 2", run_report)

            self.assertEqual(syn_status["status"], "ok")
            self.assertEqual(syn_status["bc_count"], 1)
            self.assertEqual(syn_status["matched_rows"], 1)
            self.assertEqual(syn_status["retained_rows"], 1)
            self.assertEqual(quote_status["status"], "rapx_failed")
            self.assertEqual(quote_status["last_attempt_command"], "cargo +stable rapx -bounds-db")
            self.assertEqual(quote_status["log_tail"], "error line 1\nerror line 2")
            self.assertNotIn("bc_count", quote_status)

    def test_main_can_use_fixed_crates_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            output_dir = Path(tmp_dir) / "dataset_bc"
            crates_file = Path(tmp_dir) / "crates.txt"
            crates_file.write_text("syn@2.0.117\n# comment\nquote 1.0.39\n", encoding="utf-8")

            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--top-n",
                "99",
                "--crates-file",
                str(crates_file),
                "--output-dir",
                str(output_dir),
                "--toolchain",
                "nightly-2025-12-06",
            ]

            with mock.patch.object(
                script, "fetch_popular_crates", side_effect=AssertionError("should not fetch")
            ):
                with mock.patch.object(
                    script, "download_and_extract_crate", side_effect=lambda c, v, d: d / f"{c}-{v}"
                ):
                    with mock.patch.object(
                        script,
                        "run_rapx",
                        return_value=(
                            False,
                            "rapx failed",
                            "none",
                            0.1,
                            "none",
                            "cargo rapx -bounds-db",
                            "rapx failed",
                        ),
                    ):
                        with mock.patch.object(sys, "argv", argv):
                            script.main()

            dataset_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            self.assertEqual(dataset_index["source_mode"], "fixed_crates")
            self.assertEqual(dataset_index["crates_file"], str(crates_file))
            self.assertIsNone(dataset_index["snapshot_path"])
            self.assertEqual(len(dataset_index["crates"]), 2)
            self.assertFalse((output_dir / "popular_crates_top99.json").exists())

    def test_main_offline_auto_discovers_existing_sources_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"
            sources_dir = output_dir / "sources"
            crate_dir = sources_dir / "syn-2.0.117"
            crate_dir.mkdir(parents=True, exist_ok=True)
            (crate_dir / "Cargo.toml").write_text(
                "[package]\nname='syn'\nversion='2.0.117'\n", encoding="utf-8"
            )
            (crate_dir / "bounds_checks_syn.json").write_text(
                json.dumps(
                    {
                        "bounds_checks": [
                            {
                                "location": {"file": "src/lib.rs", "line": 12},
                                "symbolic_features": {},
                                "function_context": {},
                                "call_context": {},
                            }
                        ],
                        "llvm": {
                            "reserved": {
                                "analysis": "llvm-ir-release",
                                "records": [
                                    {
                                        "file": "src/lib.rs",
                                        "line": 12,
                                        "function": "crate::f",
                                        "retained": False,
                                    }
                                ],
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(
                script, "download_and_extract_crate", side_effect=AssertionError("should not download")
            ):
                with mock.patch.object(
                    script,
                    "run_rapx",
                    return_value=(
                        True,
                        "rapx ok",
                        "nightly-2025-12-06",
                        0.2,
                        "none",
                        "cargo +nightly-2025-12-06 rapx -bounds-db",
                        "rapx ok",
                    ),
                ):
                    with mock.patch.object(sys, "argv", argv):
                        script.main()

            dataset_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            run_report_path = Path(dataset_index["run_report_path"])
            run_report = run_report_path.read_text(encoding="utf-8")
            status = json.loads(
                (output_dir / "crate_status" / "syn-2.0.117.json").read_text(encoding="utf-8")
            )
            dataset = json.loads(
                (output_dir / "bounds_checks_dataset.json").read_text(encoding="utf-8")
            )

            self.assertEqual(dataset_index["source_mode"], "offline_existing_sources")
            self.assertIsNone(dataset_index["crates_file"])
            self.assertTrue(dataset_index["offline"])
            self.assertEqual(dataset_index["sources_dir"], str(sources_dir.resolve()))
            self.assertEqual(run_report_path.parent, output_dir / "run_reports")
            self.assertEqual(status["source_origin"], "existing_sources")
            self.assertEqual(status["status"], "ok")
            self.assertEqual(dataset["metadata"]["source_mode"], "offline_existing_sources")
            self.assertEqual(dataset["metadata"]["run_report_path"], str(run_report_path))
            self.assertEqual(dataset["metadata"]["sources_dir"], str(sources_dir.resolve()))
            self.assertEqual(len(dataset["records"]), 1)
            self.assertIn("source_mode: offline_existing_sources", run_report)
            self.assertIn("- success_count: 1", run_report)
            self.assertIn("existing_sources", run_report)

    def test_main_offline_can_filter_with_crates_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"
            sources_dir = output_dir / "sources"
            syn_dir = sources_dir / "syn-2.0.117"
            syn_dir.mkdir(parents=True, exist_ok=True)
            (syn_dir / "Cargo.toml").write_text(
                "[package]\nname='syn'\nversion='2.0.117'\n", encoding="utf-8"
            )
            (syn_dir / "bounds_checks_syn.json").write_text(
                json.dumps({"bounds_checks": [], "llvm": {"reserved": {"records": []}}}),
                encoding="utf-8",
            )
            quote_dir = sources_dir / "quote-1.0.39"
            quote_dir.mkdir(parents=True, exist_ok=True)
            (quote_dir / "Cargo.toml").write_text(
                "[package]\nname='quote'\nversion='1.0.39'\n", encoding="utf-8"
            )
            (quote_dir / "bounds_checks_quote.json").write_text(
                json.dumps({"bounds_checks": [], "llvm": {"reserved": {"records": []}}}),
                encoding="utf-8",
            )
            crates_file = tmp_path / "crates.txt"
            crates_file.write_text("syn@2.0.117\n", encoding="utf-8")
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--crates-file",
                str(crates_file),
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(
                script,
                "run_rapx",
                return_value=(
                    True,
                    "rapx ok",
                    "nightly-2025-12-06",
                    0.2,
                    "none",
                    "cargo +nightly-2025-12-06 rapx -bounds-db",
                    "rapx ok",
                ),
            ):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(io.StringIO()):
                        script.main()

            dataset_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            self.assertEqual(dataset_index["status"], "ok")
            self.assertTrue(dataset_index["offline"])
            self.assertEqual(dataset_index["source_mode"], "offline_fixed_crates")
            self.assertEqual(dataset_index["crate_count"], 1)
            self.assertEqual(len(dataset_index["crates"]), 1)
            self.assertEqual(dataset_index["crates"][0]["crate"], "syn")

    def test_main_offline_fails_when_source_missing_in_sources_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"

            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(sys, "argv", argv):
                with redirect_stdout(io.StringIO()):
                    script.main()

            dataset_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            status = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            self.assertEqual(dataset_index["status"], "load_sources_failed")
            self.assertIn("no valid crates discovered", dataset_index["error"])

    def test_main_recovers_bounds_json_when_rapx_exits_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"
            sources_dir = output_dir / "sources"
            crate_dir = sources_dir / "autocfg-1.5.0"
            crate_dir.mkdir(parents=True, exist_ok=True)
            (crate_dir / "Cargo.toml").write_text(
                "[package]\nname='autocfg'\nversion='1.5.0'\n", encoding="utf-8"
            )
            (crate_dir / "bounds_checks_autocfg.json").write_text(
                json.dumps(
                    {
                        "bounds_checks": [
                            {
                                "location": {"file": "src/lib.rs", "line": 46},
                                "symbolic_features": {},
                                "function_context": {},
                                "call_context": {},
                            }
                        ],
                        "llvm": {
                            "reserved": {
                                "records": [
                                    {
                                        "file": "src/lib.rs",
                                        "line": 46,
                                        "function": "autocfg::f",
                                        "retained": True,
                                    }
                                ]
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(
                script,
                "run_rapx",
                return_value=(
                    False,
                    "RAP dumped json but cargo failed",
                    "none",
                    0.2,
                    "none",
                    "cargo rapx -bounds-db",
                    "cargo failed",
                ),
            ):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(io.StringIO()):
                        script.main()

            dataset = json.loads(
                (output_dir / "bounds_checks_dataset.json").read_text(encoding="utf-8")
            )
            dataset_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            status = json.loads(
                (output_dir / "crate_status" / "autocfg-1.5.0.json").read_text(
                    encoding="utf-8"
                )
            )
            run_report = Path(dataset_index["run_report_path"]).read_text(encoding="utf-8")

            self.assertEqual(status["status"], "ok_with_rapx_nonzero")
            self.assertFalse(status["rapx_exit_ok"])
            self.assertEqual(status["bc_count"], 1)
            self.assertEqual(dataset["metadata"]["success_count"], 1)
            self.assertEqual(dataset["metadata"]["failed_count"], 0)
            self.assertEqual(len(dataset["records"]), 1)
            self.assertEqual(dataset_index["success_count"], 1)
            self.assertEqual(dataset_index["failed_count"], 0)
            self.assertNotIn("Failures Detail", run_report)

    def test_main_classifies_proc_macro_without_bounds_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"
            sources_dir = output_dir / "sources"
            crate_dir = sources_dir / "serde_derive-1.0.228"
            crate_dir.mkdir(parents=True, exist_ok=True)
            (crate_dir / "Cargo.toml").write_text(
                "\n".join(
                    [
                        "[package]",
                        "name='serde_derive'",
                        "version='1.0.228'",
                        "",
                        "[lib]",
                        "proc-macro = true",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(
                script,
                "run_rapx",
                return_value=(
                    True,
                    "cargo finished",
                    "nightly-2025-12-06",
                    0.2,
                    "none",
                    "cargo +nightly-2025-12-06 rapx -bounds-db",
                    "cargo finished",
                ),
            ):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(io.StringIO()):
                        script.main()

            status = json.loads(
                (output_dir / "crate_status" / "serde_derive-1.0.228.json").read_text(
                    encoding="utf-8"
                )
            )
            dataset_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )

            self.assertEqual(
                status["status"], "unsupported_proc_macro_or_no_bounds_json"
            )
            self.assertTrue(status["rapx_exit_ok"])
            self.assertEqual(dataset_index["success_count"], 0)
            self.assertEqual(dataset_index["failed_count"], 1)

    def test_main_keeps_bc_json_not_found_for_non_proc_macro_success(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"
            sources_dir = output_dir / "sources"
            crate_dir = sources_dir / "plain-0.1.0"
            crate_dir.mkdir(parents=True, exist_ok=True)
            (crate_dir / "Cargo.toml").write_text(
                "[package]\nname='plain'\nversion='0.1.0'\n", encoding="utf-8"
            )
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--output-dir",
                str(output_dir),
            ]

            with mock.patch.object(
                script,
                "run_rapx",
                return_value=(
                    True,
                    "cargo finished",
                    "nightly-2025-12-06",
                    0.2,
                    "none",
                    "cargo +nightly-2025-12-06 rapx -bounds-db",
                    "cargo finished",
                ),
            ):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(io.StringIO()):
                        script.main()

            status = json.loads(
                (output_dir / "crate_status" / "plain-0.1.0.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(status["status"], "bc_json_not_found")
            self.assertTrue(status["rapx_exit_ok"])

    def test_main_writes_distinct_run_reports_for_repeated_runs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            output_dir = tmp_path / "dataset_bc"
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--offline",
                "--output-dir",
                str(output_dir),
            ]

            run_times = [
                "2026-01-01T00:00:00+00:00",
                "2026-01-01T00:00:01+00:00",
                "2026-01-01T00:00:02+00:00",
                "2026-01-01T00:00:03+00:00",
            ]

            with mock.patch.object(script, "utc_now_iso", side_effect=run_times):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(io.StringIO()):
                        script.main()

            first_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            first_report_path = Path(first_index["run_report_path"])
            self.assertTrue(first_report_path.exists())

            run_times = [
                "2026-01-01T00:01:00+00:00",
                "2026-01-01T00:01:01+00:00",
                "2026-01-01T00:01:02+00:00",
                "2026-01-01T00:01:03+00:00",
            ]
            with mock.patch.object(script, "utc_now_iso", side_effect=run_times):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(io.StringIO()):
                        script.main()

            second_index = json.loads(
                (output_dir / "dataset_index.json").read_text(encoding="utf-8")
            )
            second_report_path = Path(second_index["run_report_path"])
            self.assertTrue(second_report_path.exists())
            self.assertNotEqual(first_report_path, second_report_path)
            self.assertTrue(first_report_path.exists())
            reports = sorted((output_dir / "run_reports").glob("run_report_*.log"))
            self.assertEqual(len(reports), 2)


if __name__ == "__main__":
    unittest.main()
