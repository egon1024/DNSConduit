#!/usr/bin/env python3
"""Unit tests for finalize_release_notes.py."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from finalize_release_notes import (
    EMPTY_UNRELEASED,
    finalize,
    has_substantive_unreleased,
    insert_index_row,
    strip_unreleased_header,
)

INDEX_TEMPLATE = """# Release notes

## Releases

| Version | Summary |
| --- | --- |

_No published release notes yet. The first entry appears when the next version ships._
"""


class FinalizeReleaseNotesTests(unittest.TestCase):
    def test_strip_unreleased_header(self) -> None:
        text = """# Unreleased

## New features

- Added widgets.
"""
        self.assertEqual(
            strip_unreleased_header(text),
            "## New features\n\n- Added widgets.",
        )

    def test_placeholder_is_not_substantive(self) -> None:
        self.assertFalse(has_substantive_unreleased(EMPTY_UNRELEASED))

    def test_bullets_are_substantive(self) -> None:
        text = """# Unreleased

## Fixes

- Fixed export YAML.
"""
        self.assertTrue(has_substantive_unreleased(text))

    def test_insert_index_row_replaces_placeholder(self) -> None:
        updated = insert_index_row(INDEX_TEMPLATE, "0.14.0", "Added widgets")
        self.assertIn("| [0.14.0](0.14.0.md) | Added widgets |", updated)
        self.assertNotIn("_No published release notes yet", updated)

    def test_finalize_writes_version_and_resets_unreleased(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            unreleased = root / "unreleased.md"
            index = root / "index.md"
            index.write_text(INDEX_TEMPLATE, encoding="utf-8")
            unreleased.write_text(
                "# Unreleased\n\n## New features\n\n- Added widgets.\n",
                encoding="utf-8",
            )

            wrote = finalize(
                "0.14.0",
                root,
                "https://github.com/example/repo",
            )
            self.assertTrue(wrote)
            version_text = (root / "0.14.0.md").read_text(encoding="utf-8")
            self.assertIn("# Release notes — 0.14.0", version_text)
            self.assertIn("Added widgets", version_text)
            self.assertEqual(unreleased.read_text(encoding="utf-8"), EMPTY_UNRELEASED)
            self.assertIn("[0.14.0](0.14.0.md)", index.read_text(encoding="utf-8"))

    def test_finalize_stub_when_unreleased_empty(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "index.md").write_text(INDEX_TEMPLATE, encoding="utf-8")
            (root / "unreleased.md").write_text(EMPTY_UNRELEASED, encoding="utf-8")

            finalize("0.14.1", root, "https://github.com/example/repo")
            version_text = (root / "0.14.1.md").read_text(encoding="utf-8")
            self.assertIn("Maintenance release", version_text)

    def test_finalize_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "index.md").write_text(INDEX_TEMPLATE, encoding="utf-8")
            (root / "unreleased.md").write_text(EMPTY_UNRELEASED, encoding="utf-8")

            self.assertTrue(finalize("0.15.0", root, "https://github.com/example/repo"))
            unreleased_after_first = (root / "unreleased.md").read_text(encoding="utf-8")
            self.assertFalse(finalize("0.15.0", root, "https://github.com/example/repo"))
            self.assertEqual(
                (root / "unreleased.md").read_text(encoding="utf-8"),
                unreleased_after_first,
            )


if __name__ == "__main__":
    unittest.main()
