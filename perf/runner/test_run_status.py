"""Unit tests for run status / ETA helpers."""

from __future__ import annotations

import unittest
from datetime import datetime, timedelta, timezone

from perf.runner.api import RunProgressEvent
from perf.tui.run_status import RunStatusModel, format_duration


class RunStatusModelTests(unittest.TestCase):
    def test_idle_defaults(self):
        m = RunStatusModel()
        self.assertEqual(m.phase, "idle")
        self.assertEqual(m.fraction(), 0.0)
        self.assertIsNone(m.eta_at())

    def test_progress_and_eta(self):
        m = RunStatusModel()
        m.mark_start(
            cycles=2,
            scenarios_per_cycle=4,
            planned_cell_seconds=10.0,
            detail="go",
        )
        self.assertEqual(m.phase, "running")
        self.assertEqual(m.total_units(), 8)
        # Pretend one cell done over 10s.
        m.started_at = datetime.now(timezone.utc) - timedelta(seconds=10)
        m.observe(
            RunProgressEvent(
                kind="scenario_done",
                scenario_id="a",
                index=1,
                total=4,
                cycle=1,
                cycles=2,
            )
        )
        self.assertEqual(m.completed_units, 1)
        self.assertAlmostEqual(m.fraction(), 1 / 8)
        eta = m.eta_at()
        self.assertIsNotNone(eta)
        # ~70s remaining at 10s/cell for 7 left → eta ~70s from now
        remaining = (eta - datetime.now(timezone.utc)).total_seconds()
        self.assertGreater(remaining, 40)
        self.assertLess(remaining, 120)

    def test_complete(self):
        m = RunStatusModel()
        m.mark_start(cycles=1, scenarios_per_cycle=2, planned_cell_seconds=5, detail="")
        m.mark_complete(ok=True)
        self.assertEqual(m.phase, "complete")
        self.assertEqual(m.fraction(), 1.0)
        self.assertIsNotNone(m.ended_at)

    def test_format_duration(self):
        self.assertEqual(format_duration(65), "1m 05s")
        self.assertEqual(format_duration(5), "5s")


if __name__ == "__main__":
    unittest.main()
