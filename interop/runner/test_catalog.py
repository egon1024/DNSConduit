"""Unit tests for interop catalog sorting and fingerprint."""

from __future__ import annotations

import unittest

from interop.runner.catalog import Peer, sorted_peers
from interop.runner.fingerprint import compute_inputs_fingerprint
from interop.runner.generate_matrix import (
    executed_status_phrase,
    outcome_cell,
    profile_block_all_skips,
    publisher_slug,
)


class CaseMatrixFieldTests(unittest.TestCase):
    def test_conduit_cases_tagged(self):
        from interop.runner.cases import load_cases

        by_id = {c.id: c for c in load_cases()}
        self.assertEqual(by_id["basic-a-forward"].matrix, "peer")
        self.assertTrue(by_id["dataplane-split-io-forward"].is_conduit_matrix)
        self.assertIn(
            "thekelleys-dnsmasq-2.90",
            by_id["dataplane-split-io-forward"].applicability.get("peers", []),
        )


class CatalogSortTests(unittest.TestCase):
    def test_publisher_alphabetical(self):
        peers = [
            Peer("z", "PowerDNS", "Recursor", "5.4", "recursive", "img", family="pdns-recursor"),
            Peer("a", "CZ.NIC", "Knot DNS", "3.3", "auth", "img", family="knot"),
            Peer("b", "ISC", "BIND", "9.18", "auth", "img", family="bind"),
            Peer("y", "PowerDNS", "Recursor", "5.3", "recursive", "img", family="pdns-recursor"),
        ]
        ids = [p.id for p in sorted_peers(peers)]
        self.assertEqual(ids, ["a", "b", "y", "z"])


class PublisherSlugTests(unittest.TestCase):
    def test_slugs(self):
        self.assertEqual(publisher_slug("CZ.NIC"), "cz-nic")
        self.assertEqual(publisher_slug("NLnet Labs"), "nlnet-labs")
        self.assertEqual(publisher_slug("PowerDNS"), "powerdns")


class OutcomeCellTests(unittest.TestCase):
    def test_known_outcomes_use_css_classes(self):
        for outcome in ("pass", "fail", "skip", "characterized"):
            html = outcome_cell(outcome)
            self.assertIn(f'class="interop-outcome interop-outcome--{outcome}"', html)
            self.assertIn(f">{outcome}</span>", html)

    def test_unknown_outcome_passthrough(self):
        self.assertEqual(outcome_cell("weird"), "weird")


class ExecutedStatusPhraseTests(unittest.TestCase):
    def test_all_pass(self):
        cells = [
            {"outcome": "pass"},
            {"outcome": "skip"},
            {"outcome": "pass"},
        ]
        self.assertEqual(executed_status_phrase(cells), "All executed cases passed")

    def test_failures(self):
        cells = [{"outcome": "pass"}, {"outcome": "fail"}, {"outcome": "skip"}]
        self.assertEqual(
            executed_status_phrase(cells),
            "Failures present (1 fail)",
        )

    def test_characterized_without_fail(self):
        cells = [{"outcome": "pass"}, {"outcome": "characterized"}, {"outcome": "skip"}]
        self.assertEqual(
            executed_status_phrase(cells),
            "No failures; 1 characterized",
        )

    def test_only_skips(self):
        self.assertEqual(
            executed_status_phrase([{"outcome": "skip"}]),
            "No executed cases (all out of scope)",
        )


class ProfileBlockAllSkipsTests(unittest.TestCase):
    def test_all_skips(self):
        outcomes = ["skip", "skip", "skip"]
        self.assertTrue(profile_block_all_skips(outcomes))

    def test_has_pass(self):
        self.assertFalse(profile_block_all_skips(["skip", "pass"]))

    def test_empty_not_all_skips(self):
        # No cells → don't treat as collapse-worthy skip plane.
        self.assertFalse(profile_block_all_skips([]))

    def test_emdash_prevents_collapse(self):
        self.assertFalse(profile_block_all_skips(["skip", None]))


class FingerprintTests(unittest.TestCase):
    def test_fingerprint_format(self):
        fp = compute_inputs_fingerprint()
        self.assertTrue(fp.startswith("sha256:"))
        self.assertEqual(len(fp), len("sha256:") + 64)


if __name__ == "__main__":
    unittest.main()
