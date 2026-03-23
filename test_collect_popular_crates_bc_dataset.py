import io
import json
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
                "10",
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


if __name__ == "__main__":
    unittest.main()
