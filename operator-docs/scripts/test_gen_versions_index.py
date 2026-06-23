#!/usr/bin/env python3
"""Unit tests for gen_versions_index.py."""

from __future__ import annotations

import unittest

from gen_versions_index import merge_current_version, render_versions_md


class GenVersionsIndexTests(unittest.TestCase):
    def test_strip_latest_from_previous_version_when_assigning_to_new_release(self) -> None:
        entries = [
            {"version": "0.13.0", "title": "0.13.0", "aliases": ["latest"]},
            {"version": "0.12.0", "title": "0.12.0", "aliases": []},
        ]
        merge_current_version(entries, "0.14.0", ["latest"])

        by_version = {e["version"]: e for e in entries}
        self.assertEqual(by_version["0.13.0"]["aliases"], [])
        self.assertEqual(by_version["0.14.0"]["aliases"], ["latest"])

    def test_aliases_table_points_latest_at_new_version(self) -> None:
        entries = [
            {"version": "0.13.0", "title": "0.13.0", "aliases": ["latest"]},
        ]
        merge_current_version(entries, "0.14.0", ["latest"])
        md = render_versions_md(entries, "https://example.test/", "0.14.0")
        self.assertIn("| `latest` | `0.14.0` |", md)
        self.assertNotIn("| `latest` | `0.13.0` |", md)


if __name__ == "__main__":
    unittest.main()
