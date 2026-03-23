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
    def test_main_writes_manifest_when_fetch_fails(self) -> None:
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
            with mock.patch.object(script, "fetch_popular_crates", side_effect=OSError("Network is unreachable")):
                with mock.patch.object(sys, "argv", argv):
                    with redirect_stdout(buf):
                        script.main()

            manifest_path = output_dir / "manifest.json"
            self.assertTrue(manifest_path.exists())
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["status"], "fetch_popular_crates_failed")
            self.assertIn("Network is unreachable", manifest["error"])
            self.assertEqual(manifest["processed"], [])
            self.assertEqual(manifest["total_rows"], 0)

    def test_run_rapx_fallbacks_include_stable_and_nightly(self) -> None:
        calls = []

        def fake_run(cmd, **kwargs):
            calls.append(cmd)
            if cmd[:2] == ["cargo", "+stable"]:
                return subprocess.CompletedProcess(cmd, 0, stdout="ok-stable")
            return subprocess.CompletedProcess(cmd, 1, stdout="failed")

        with tempfile.TemporaryDirectory() as tmp_dir:
            crate_dir = Path(tmp_dir)
            with mock.patch.object(script, "_detect_crate_toolchain", return_value=None):
                with mock.patch.object(script.subprocess, "run", side_effect=fake_run):
                    ok, output, used_toolchain = script.run_rapx(
                        crate_dir, "1.71.0", timeout_sec=30
                    )

        self.assertTrue(ok)
        self.assertEqual(used_toolchain, "stable")
        self.assertIn("$ cargo +1.71.0 rapx -bounds-db", output)
        self.assertIn("$ cargo +stable rapx -bounds-db", output)
        self.assertEqual(calls[0], ["cargo", "+1.71.0", "rapx", "-bounds-db"])
        self.assertEqual(calls[1], ["cargo", "+stable", "rapx", "-bounds-db"])

    def test_main_preserves_downloaded_sources_locally(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            output_dir = Path(tmp_dir) / "dataset_bc"
            argv = [
                "collect_popular_crates_bc_dataset.py",
                "--top-n",
                "1",
                "--output-dir",
                str(output_dir),
                "--toolchain",
                "nightly-2025-12-06",
            ]

            def fake_download(crate: str, version: str, dst_dir: Path) -> Path:
                saved = dst_dir / f"{crate}-{version}"
                saved.mkdir(parents=True, exist_ok=True)
                (saved / "Cargo.toml").write_text("[package]\nname='x'\nversion='0.1.0'\n", encoding="utf-8")
                return saved

            with mock.patch.object(script, "fetch_popular_crates", return_value=[{"name": "syn", "version": "2.0.117"}]):
                with mock.patch.object(script, "download_and_extract_crate", side_effect=fake_download):
                    with mock.patch.object(script, "run_rapx", return_value=(False, "rapx failed", "none")):
                        with mock.patch.object(sys, "argv", argv):
                            script.main()

            manifest = json.loads((output_dir / "manifest.json").read_text(encoding="utf-8"))
            processed = manifest["processed"]
            self.assertEqual(len(processed), 1)
            self.assertEqual(processed[0]["status"], "rapx_failed")
            source_dir = Path(processed[0]["source_dir"])
            self.assertTrue(source_dir.exists())
            self.assertTrue(str(source_dir).startswith(str((output_dir / "sources").resolve())))

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

            with mock.patch.object(script, "fetch_popular_crates", side_effect=AssertionError("should not fetch")):
                with mock.patch.object(script, "download_and_extract_crate", side_effect=lambda c, v, d: d / f"{c}-{v}"):
                    with mock.patch.object(script, "run_rapx", return_value=(False, "rapx failed", "none")):
                        with mock.patch.object(sys, "argv", argv):
                            script.main()

            manifest = json.loads((output_dir / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["source_mode"], "fixed_crates")
            self.assertEqual(manifest["crates_file"], str(crates_file))
            self.assertEqual(len(manifest["processed"]), 2)
            self.assertEqual(manifest["processed"][0]["crate"], "syn")
            self.assertEqual(manifest["processed"][1]["crate"], "quote")


if __name__ == "__main__":
    unittest.main()
