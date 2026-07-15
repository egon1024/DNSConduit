"""Unit tests for interop oracle property / parity / differential checks."""

from __future__ import annotations

import unittest

from interop.runner.compose import parse_dig
from interop.runner.oracles import QueryResult, evaluate_oracles


SAMPLE_DIG = """\
;; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 12345
;; flags: qr aa rd; QUERY: 1, ANSWER: 2, AUTHORITY: 1, ADDITIONAL: 1

;; OPT PSEUDOSECTION:
; EDNS: version: 0, flags:; udp: 1232
;; ANSWER SECTION:
alias.example.test. 3600 IN CNAME www.example.test.
www.example.test. 3600 IN A 192.0.2.10

;; AUTHORITY SECTION:
example.test. 3600 IN NS ns1.example.test.

;; ADDITIONAL SECTION:
ns1.example.test. 3600 IN A 192.0.2.53
"""


class ParseDigTests(unittest.TestCase):
    def test_parse_flags_edns_and_cname(self):
        result = parse_dig(SAMPLE_DIG)
        self.assertEqual(result.rcode, "NOERROR")
        self.assertTrue(result.flags.get("aa"))
        self.assertFalse(result.flags.get("tc"))
        self.assertEqual(result.ancount, 2)
        self.assertEqual(result.nscount, 1)
        self.assertEqual(result.arcount, 1)
        self.assertEqual(result.edns_udp_size, 1232)
        self.assertTrue(result.has_cname)
        self.assertEqual(result.answer_types, {"CNAME", "A"})


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

    def test_nodata(self):
        via = QueryResult(rcode="NOERROR", ancount=0, answers=[], flags={"aa": True})
        outcome, _ = evaluate_oracles(
            [{"kind": "property", "checks": ["nodata"]}],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_rcode_refused(self):
        via = QueryResult(rcode="REFUSED", ancount=0, answers=[])
        outcome, _ = evaluate_oracles(
            [{"kind": "property", "checks": ["rcode-refused"]}],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_has_cname_and_answer_types(self):
        via = QueryResult(
            rcode="NOERROR",
            ancount=2,
            answers=[
                {"name": "alias.", "type": "CNAME", "rdata": "www."},
                {"name": "www.", "type": "A", "rdata": "192.0.2.10"},
            ],
        )
        outcome, _ = evaluate_oracles(
            [
                {
                    "kind": "property",
                    "checks": ["has-cname", "answer-types"],
                    "answer_types": ["CNAME", "A"],
                }
            ],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

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


class ParityAndDifferentialTests(unittest.TestCase):
    def test_parity_aa_and_answer_types(self):
        via = QueryResult(
            rcode="NOERROR",
            ancount=1,
            answers=[{"name": "www.", "type": "A", "rdata": "192.0.2.10"}],
            flags={"aa": True},
        )
        direct = QueryResult(
            rcode="NOERROR",
            ancount=1,
            answers=[{"name": "www.", "type": "A", "rdata": "192.0.2.10"}],
            flags={"aa": True},
        )
        outcome, _ = evaluate_oracles(
            [{"kind": "parity", "compare": ["rcode", "ancount", "aa", "answer-types"]}],
            via_conduit=via,
            direct=direct,
            peer_id="peer",
        )
        self.assertEqual(outcome, "pass")

    def test_parity_aa_mismatch(self):
        via = QueryResult(rcode="NOERROR", ancount=0, answers=[], flags={"aa": False})
        direct = QueryResult(rcode="NOERROR", ancount=0, answers=[], flags={"aa": True})
        outcome, detail = evaluate_oracles(
            [{"kind": "parity", "compare": ["aa"]}],
            via_conduit=via,
            direct=direct,
            peer_id="peer",
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("aa", detail.lower())

    def test_differential_cname_only(self):
        via = QueryResult(
            rcode="NOERROR",
            ancount=1,
            answers=[{"name": "alias.", "type": "CNAME", "rdata": "target."}],
        )
        outcome, detail = evaluate_oracles(
            [
                {
                    "kind": "differential",
                    "expect": {
                        "default": {
                            "rcode": "NOERROR",
                            "ancount": 1,
                            "has_cname": True,
                            "answer_types": ["CNAME"],
                        }
                    },
                }
            ],
            via_conduit=via,
            direct=None,
            peer_id="nlnetlabs-unbound-1.22",
        )
        self.assertEqual(outcome, "characterized")
        self.assertIn("differential", detail.lower())

    def test_differential_flags(self):
        via = QueryResult(rcode="REFUSED", ancount=0, answers=[], flags={"aa": False})
        outcome, _ = evaluate_oracles(
            [
                {
                    "kind": "differential",
                    "expect": {"default": {"rcode": "REFUSED", "flags": {"aa": False}}},
                }
            ],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "characterized")


class PeerQueryCountOracleTests(unittest.TestCase):
    def test_pass_when_delta_matches_expect(self):
        via = QueryResult(rcode="NOERROR", ancount=1, answers=[{"rdata": "192.0.2.20"}])
        outcome, detail = evaluate_oracles(
            [
                {"kind": "property", "checks": ["rcode-noerror"]},
                {
                    "kind": "peer-query-count",
                    "expect": 1,
                    "qname": "www.smoke.test.",
                    "qtype": "A",
                },
            ],
            via_conduit=via,
            via_steps=[via, via],
            direct=None,
            peer_id="peer",
            peer_query_deltas={("www.smoke.test", "A"): 1},
        )
        self.assertEqual(outcome, "pass")
        self.assertIn("peer-query-count", detail)

    def test_fail_when_warm_path_also_hit_peer(self):
        via = QueryResult(rcode="NOERROR", ancount=1, answers=[{"rdata": "192.0.2.20"}])
        outcome, detail = evaluate_oracles(
            [
                {
                    "kind": "peer-query-count",
                    "expect": 1,
                    "qname": "www.smoke.test.",
                    "qtype": "A",
                }
            ],
            via_conduit=via,
            via_steps=[via, via],
            direct=None,
            peer_id="peer",
            peer_query_deltas={("www.smoke.test", "A"): 2},
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("peer-query-count", detail.lower())
        self.assertIn("want 1", detail)

    def test_fail_when_delta_missing(self):
        via = QueryResult(rcode="NXDOMAIN", ancount=0, answers=[])
        outcome, detail = evaluate_oracles(
            [
                {
                    "kind": "peer-query-count",
                    "expect": 1,
                    "qname": "missing.nxcache.test.",
                    "qtype": "A",
                }
            ],
            via_conduit=via,
            direct=None,
            peer_id="peer",
        )
        self.assertEqual(outcome, "fail")
        self.assertIn("unavailable", detail.lower())


if __name__ == "__main__":
    unittest.main()
