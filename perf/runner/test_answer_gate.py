"""Unit tests for the successful-answer gate on forward-path measurements."""

from __future__ import annotations

import unittest

from perf.runner.execute import (
    DEFAULT_MIN_ANSWER_OK_PERCENT,
    _apply_answer_gate,
    answer_gate_settings,
    answer_ok_percent,
)
from perf.runner.publish import assert_no_invalid_scenarios, merge_median_documents


def _scenario(status: str = "ok", qps: float = 100.0) -> dict:
    return {
        "id": "unit-cell",
        "suite": "scale",
        "status": status,
        "metrics": {"achieved_qps": qps},
    }


def _round_doc(scenarios: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "generated_at": "2026-08-01T00:00:00Z",
        "lab_profile": {"id": "unit", "cpu_model": "unit"},
        "provenance": {"conduit_path": "conduit", "conduit_version": "0"},
        "scenarios": scenarios,
    }


class AnswerOkPercentTests(unittest.TestCase):
    def test_all_successful(self):
        self.assertAlmostEqual(answer_ok_percent({"NOERROR": 500}), 100.0)

    def test_mixed(self):
        share = answer_ok_percent({"NOERROR": 100, "SERVFAIL": 900})
        self.assertAlmostEqual(share or 0.0, 10.0)

    def test_missing_breakdown_is_unknown(self):
        self.assertIsNone(answer_ok_percent({}))
        self.assertIsNone(answer_ok_percent(None))

    def test_alternate_expected_rcode(self):
        share = answer_ok_percent({"NXDOMAIN": 4, "NOERROR": 1}, expected_rcode="NXDOMAIN")
        self.assertAlmostEqual(share or 0.0, 80.0)


class AnswerGateSettingsTests(unittest.TestCase):
    def test_defaults(self):
        expected, threshold = answer_gate_settings({})
        self.assertEqual(expected, "NOERROR")
        self.assertEqual(threshold, DEFAULT_MIN_ANSWER_OK_PERCENT)

    def test_disabled_by_zero(self):
        _expected, threshold = answer_gate_settings({"min_answer_ok_percent": 0})
        self.assertIsNone(threshold)

    def test_custom_threshold_and_rcode(self):
        expected, threshold = answer_gate_settings(
            {"expect_rcode": "nxdomain", "min_answer_ok_percent": 90}
        )
        self.assertEqual(expected, "NXDOMAIN")
        self.assertEqual(threshold, 90.0)


class ApplyAnswerGateTests(unittest.TestCase):
    def test_clean_cell_stays_ok(self):
        result = {"status": "ok"}
        metrics = {"achieved_qps": 120000.0, "response_codes": {"NOERROR": 1000}}
        _apply_answer_gate(result, recipe={}, metrics=metrics)
        self.assertEqual(result["status"], "ok")
        self.assertAlmostEqual(metrics["answer_ok_percent"], 100.0)

    def test_servfail_storm_is_invalid(self):
        result = {"status": "ok"}
        metrics = {
            "achieved_qps": 745923.0,
            "response_codes": {"NOERROR": 80, "SERVFAIL": 920},
        }
        _apply_answer_gate(result, recipe={}, metrics=metrics)
        self.assertEqual(result["status"], "invalid")
        self.assertIn("SERVFAIL 920", result["error"])
        self.assertAlmostEqual(metrics["answer_ok_percent"], 8.0)

    def test_gate_opt_out_keeps_cell_ok(self):
        result = {"status": "ok"}
        metrics = {"response_codes": {"NOERROR": 80, "SERVFAIL": 920}}
        _apply_answer_gate(result, recipe={"min_answer_ok_percent": 0}, metrics=metrics)
        self.assertEqual(result["status"], "ok")
        self.assertAlmostEqual(metrics["answer_ok_percent"], 8.0)


class PromoteGateTests(unittest.TestCase):
    def test_promote_refuses_invalid_scenario(self):
        doc = _round_doc([_scenario(status="invalid")])
        doc["scenarios"][0]["error"] = "answer gate: 8.02% NOERROR"
        with self.assertRaises(ValueError) as ctx:
            assert_no_invalid_scenarios(doc)
        self.assertIn("unit-cell", str(ctx.exception))

    def test_promote_allows_clean_document(self):
        assert_no_invalid_scenarios(_round_doc([_scenario()]))

    def test_median_merge_keeps_boundary_cell_invalid(self):
        merged = merge_median_documents(
            [
                _round_doc([_scenario(qps=100.0)]),
                _round_doc([_scenario(status="invalid", qps=900.0)]),
                _round_doc([_scenario(qps=110.0)]),
            ]
        )
        self.assertEqual(merged["scenarios"][0]["status"], "invalid")


if __name__ == "__main__":
    unittest.main()
