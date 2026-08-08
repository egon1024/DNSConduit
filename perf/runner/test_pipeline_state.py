"""Unit tests for pipeline sync helpers."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from perf.runner.api import file_fingerprint
from perf.tui.pipeline_state import PipelineState, compare_fingerprints


class CompareFingerprintsTests(unittest.TestCase):
    def test_unknown_when_missing(self):
        self.assertEqual(compare_fingerprints(None, "abc"), "unknown")
        self.assertEqual(compare_fingerprints("abc", None), "unknown")

    def test_in_sync_and_stale(self):
        self.assertEqual(compare_fingerprints("a", "a"), "in_sync")
        self.assertEqual(compare_fingerprints("a", "b"), "stale")


class PipelineStateGenerateSyncTests(unittest.TestCase):
    def test_generate_sync_uses_stamp(self):
        with tempfile.TemporaryDirectory() as tmp:
            ref = Path(tmp) / "thin-spine.json"
            ref.write_text('{"scenarios": []}\n', encoding="utf-8")
            fp = file_fingerprint(ref)
            assert fp is not None
            state = PipelineState()
            with mock.patch(
                "perf.tui.pipeline_state.resolve_latest_reference_path",
                return_value=ref,
            ), mock.patch(
                "perf.tui.pipeline_state.read_source_reference_stamp",
                return_value={"path": str(ref), "fingerprint": fp},
            ):
                self.assertEqual(state.generate_sync(), "in_sync")
            with mock.patch(
                "perf.tui.pipeline_state.resolve_latest_reference_path",
                return_value=ref,
            ), mock.patch(
                "perf.tui.pipeline_state.read_source_reference_stamp",
                return_value={"path": str(ref), "fingerprint": "other"},
            ):
                self.assertEqual(state.generate_sync(), "stale")


if __name__ == "__main__":
    unittest.main()
