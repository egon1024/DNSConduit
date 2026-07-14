"""Unit tests for interop catalog sorting and fingerprint."""

from __future__ import annotations

import unittest

from interop.runner.catalog import Peer, sorted_peers
from interop.runner.fingerprint import compute_inputs_fingerprint
from interop.runner.generate_matrix import outcome_cell, publisher_slug


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


class FingerprintTests(unittest.TestCase):
    def test_fingerprint_format(self):
        fp = compute_inputs_fingerprint()
        self.assertTrue(fp.startswith("sha256:"))
        self.assertEqual(len(fp), len("sha256:") + 64)


if __name__ == "__main__":
    unittest.main()
