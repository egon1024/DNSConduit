"""Unit tests for TUI scope selection helpers."""

from __future__ import annotations

import unittest

from perf.tui.scope import (
    ScopeSelection,
    catalog_suites,
    default_merge_sources,
    default_promote_source,
    default_scope,
)


class ScopeSelectionTests(unittest.TestCase):
    def test_default_is_publish_set(self):
        sel = default_scope()
        self.assertTrue(sel.publish_set)
        self.assertEqual(sel.study_ids, [])
        self.assertIn("Publish set", sel.summary())
        self.assertEqual(sel.to_run_kwargs(), {"publish_set": True})

    def test_publish_set_kwargs(self):
        sel = ScopeSelection(publish_set=True, study_ids=["x"])
        kwargs = sel.to_run_kwargs()
        self.assertTrue(kwargs["publish_set"])
        self.assertNotIn("study_ids", kwargs)

    def test_empty(self):
        self.assertTrue(ScopeSelection().is_empty())
        self.assertFalse(default_scope().is_empty())

    def test_catalog_suites_nonempty(self):
        suites = catalog_suites()
        self.assertIn("scale", suites)

    def test_default_merge_sources_match_three_cycles(self):
        paths = default_merge_sources().split()
        self.assertEqual(len(paths), 3)
        self.assertTrue(paths[0].endswith("r1.json"))
        self.assertTrue(paths[2].endswith("r3.json"))
        self.assertEqual(default_promote_source().endswith("median.json"), True)


if __name__ == "__main__":
    unittest.main()
