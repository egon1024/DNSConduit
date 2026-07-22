#!/usr/bin/env python3
"""Unit tests for merge_release_dates_json.py."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from merge_release_dates_json import merge_release_dates_maps

SCRIPTS = Path(__file__).resolve().parent


class MergeReleaseDatesMapsTests(unittest.TestCase):
    def test_later_input_wins_on_conflict(self) -> None:
        merged = merge_release_dates_maps(
            {"1.0.0": "2026-07-18", "0.20.0": "2026-07-18"},
            {"1.1.0": "2026-07-22", "1.0.0": "2026-07-19"},
        )
        self.assertEqual(
            merged,
            {
                "1.0.0": "2026-07-19",
                "0.20.0": "2026-07-18",
                "1.1.0": "2026-07-22",
            },
        )

    def test_empty_layers(self) -> None:
        self.assertEqual(merge_release_dates_maps(), {})
        self.assertEqual(merge_release_dates_maps({}), {})

    def test_cli_keeps_existing_when_incoming_is_older_subset(self) -> None:
        """Redeploying 1.0.0 must not erase 1.1.0 already on gh-pages."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            published = root / "published.json"
            tag = root / "tag.json"
            out = root / "out.json"
            published.write_text(
                json.dumps({"1.1.0": "2026-07-22", "1.0.0": "2026-07-18"}),
                encoding="utf-8",
            )
            tag.write_text(
                json.dumps({"1.0.0": "2026-07-18", "0.20.0": "2026-07-18"}),
                encoding="utf-8",
            )
            # published first, tag second → tag wins overlaps; published keeps 1.1.0
            # Wait: if tag is second, 1.1.0 stays from published (not in tag), 1.0.0 from tag.
            # Correct order for deploy: published (base) then checkout (incoming wins overlaps).
            proc = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPTS / "merge_release_dates_json.py"),
                    "--input",
                    str(published),
                    "--input",
                    str(tag),
                    "--output",
                    str(out),
                ],
                cwd=str(SCRIPTS),
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertIn("Wrote 3", proc.stderr)
            data = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(data["1.1.0"], "2026-07-22")
            self.assertEqual(data["1.0.0"], "2026-07-18")
            self.assertEqual(data["0.20.0"], "2026-07-18")
            # Newest semver first in file order.
            self.assertEqual(list(data.keys())[:2], ["1.1.0", "1.0.0"])

    def test_cli_skips_missing_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            only = root / "only.json"
            out = root / "out.json"
            only.write_text(json.dumps({"1.1.0": "2026-07-22"}), encoding="utf-8")
            proc = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPTS / "merge_release_dates_json.py"),
                    "--input",
                    str(root / "missing.json"),
                    "--input",
                    str(only),
                    "--output",
                    str(out),
                ],
                cwd=str(SCRIPTS),
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertIn("skip missing", proc.stderr)
            self.assertEqual(json.loads(out.read_text(encoding="utf-8")), {"1.1.0": "2026-07-22"})


if __name__ == "__main__":
    unittest.main()
