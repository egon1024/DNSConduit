"""Unit tests for interop oracle property checks."""

from __future__ import annotations

import unittest

from interop.runner.oracles import QueryResult, evaluate_oracles


class PropertyOracleTests(unittest.TestCase):
    def test_rcode_nxdomain(self):
        via = QueryResult(rcode="NXDOMAIN", ancount=0, answers=[])
        outcome, _ = evaluate_oracles(
            [{"kind": "property", "checks": ["rcode-nxdomain"]}],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_rcode_nxdomain_fail(self):
        via = QueryResult(rcode="NOERROR", ancount=0, answers=[])
        outcome, detail = evaluate_oracles(
            [{"kind": "property", "checks": ["rcode-nxdomain"]}],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("NXDOMAIN", detail)

    def test_no_answer_timeout(self):
        via = QueryResult(rcode="TIMEOUT", ancount=0, answers=[])
        outcome, _ = evaluate_oracles(
            [{"kind": "property", "checks": ["no-answer"]}],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_no_answer_rejects_success(self):
        via = QueryResult(
            rcode="NOERROR",
            ancount=1,
            answers=[{"name": "a.", "type": "A", "rdata": "1.2.3.4"}],
        )
        outcome, detail = evaluate_oracles(
            [{"kind": "property", "checks": ["no-answer"]}],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("no answer", detail.lower())

    def test_answer_rdata_set(self):
        via = QueryResult(
            rcode="NOERROR",
            ancount=3,
            answers=[
                {"name": "r.", "type": "A", "rdata": "192.0.2.83"},
                {"name": "r.", "type": "A", "rdata": "192.0.2.81"},
                {"name": "r.", "type": "A", "rdata": "192.0.2.82"},
            ],
        )
        outcome, _ = evaluate_oracles(
            [
                {
                    "kind": "property",
                    "checks": ["rcode-noerror", "answer-rdata-set"],
                    "answer_rdata": ["192.0.2.81", "192.0.2.82", "192.0.2.83"],
                }
            ],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_answer_rdata_set_mismatch(self):
        via = QueryResult(
            rcode="NOERROR",
            ancount=1,
            answers=[{"name": "r.", "type": "A", "rdata": "192.0.2.1"}],
        )
        outcome, detail = evaluate_oracles(
            [
                {
                    "kind": "property",
                    "checks": ["answer-rdata-set"],
                    "answer_rdata": ["192.0.2.81", "192.0.2.82"],
                }
            ],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("rdata", detail.lower())

    def test_sequence_answer_order_varies(self):
        a = QueryResult(
            rcode="NOERROR",
            ancount=2,
            answers=[
                {"name": "r.", "type": "A", "rdata": "192.0.2.81"},
                {"name": "r.", "type": "A", "rdata": "192.0.2.82"},
            ],
        )
        b = QueryResult(
            rcode="NOERROR",
            ancount=2,
            answers=[
                {"name": "r.", "type": "A", "rdata": "192.0.2.82"},
                {"name": "r.", "type": "A", "rdata": "192.0.2.81"},
            ],
        )
        outcome, _ = evaluate_oracles(
            [
                {"kind": "property", "checks": ["rcode-noerror"]},
                {"kind": "sequence", "checks": ["answer-order-varies"]},
            ],
            via_conduit=b,
            via_steps=[a, a, b, a],
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_sequence_answer_order_varies_fail(self):
        a = QueryResult(
            rcode="NOERROR",
            ancount=2,
            answers=[
                {"name": "r.", "type": "A", "rdata": "192.0.2.81"},
                {"name": "r.", "type": "A", "rdata": "192.0.2.82"},
            ],
        )
        outcome, detail = evaluate_oracles(
            [{"kind": "sequence", "checks": ["answer-order-varies"]}],
            via_conduit=a,
            via_steps=[a, a, a],
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("order", detail.lower())


if __name__ == "__main__":
    unittest.main()
