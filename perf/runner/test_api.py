"""Unit tests for perf.runner.api facade helpers."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from perf.runner.api import (
    FacadeError,
    file_fingerprint,
    merge_median,
    render_run,
    resolve_latest_reference_path,
)


class FileFingerprintTests(unittest.TestCase):
    def test_missing_returns_none(self):
        self.assertIsNone(file_fingerprint(Path("/no/such/file-xyz.json")))

    def test_same_bytes_same_hash(self):
        with tempfile.TemporaryDirectory() as tmp:
            a = Path(tmp) / "a.json"
            b = Path(tmp) / "b.json"
            a.write_text('{"x": 1}\n', encoding="utf-8")
            b.write_text('{"x": 1}\n', encoding="utf-8")
            self.assertEqual(file_fingerprint(a), file_fingerprint(b))

    def test_different_bytes_different_hash(self):
        with tempfile.TemporaryDirectory() as tmp:
            a = Path(tmp) / "a.json"
            b = Path(tmp) / "b.json"
            a.write_text('{"x": 1}\n', encoding="utf-8")
            b.write_text('{"x": 2}\n', encoding="utf-8")
            self.assertNotEqual(file_fingerprint(a), file_fingerprint(b))


class FacadeCallableTests(unittest.TestCase):
    def test_merge_median_requires_two_sources(self):
        with self.assertRaises(FacadeError):
            merge_median([Path("/tmp/one.json")])

    def test_render_run_rejects_unknown_format(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "r.json"
            p.write_text("{}", encoding="utf-8")
            with self.assertRaises(FacadeError):
                render_run(p, "not-a-format")

    def test_resolve_latest_reference_path(self):
        path = resolve_latest_reference_path()
        # Repo may or may not have a pointer; if present it must exist.
        if path is not None:
            self.assertTrue(path.is_file())


if __name__ == "__main__":
    unittest.main()
