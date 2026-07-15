"""Unit tests for dnsmasq peer query-log counting (cache hit proof)."""

from __future__ import annotations

import unittest

from interop.runner.peer_query_count import count_dnsmasq_queries


SAMPLE_LOG = """\
dnsmasq[1]: started, version 2.90
dnsmasq[1]: query[A] readiness-probe.invalid from 172.30.97.1
dnsmasq[1]: query[A] www.smoke.test from 172.30.97.1
dnsmasq[1]: /peer-config/hosts www.smoke.test is 192.0.2.20
dnsmasq[1]: query[A] www.smoke.test from 172.30.97.20
dnsmasq[1]: /peer-config/hosts www.smoke.test is 192.0.2.20
dnsmasq[1]: query[AAAA] www.smoke.test from 172.30.97.20
dnsmasq[1]: query[A] missing.nxcache.test from 172.30.97.20
dnsmasq[1]: config missing.nxcache.test is NXDOMAIN
dnsmasq[1]: query[A] WWW.SMOKE.TEST from 172.30.97.20
"""


class CountDnsmasqQueriesTests(unittest.TestCase):
    def test_counts_matching_qname_and_qtype(self):
        # Three A queries for www.smoke.test (host readiness + conduit + trailing-dot casing).
        self.assertEqual(count_dnsmasq_queries(SAMPLE_LOG, "www.smoke.test.", "A"), 3)

    def test_ignores_other_qtypes(self):
        self.assertEqual(count_dnsmasq_queries(SAMPLE_LOG, "www.smoke.test", "AAAA"), 1)

    def test_counts_nxdomain_name(self):
        self.assertEqual(count_dnsmasq_queries(SAMPLE_LOG, "missing.nxcache.test.", "A"), 1)

    def test_optional_from_ip_filter(self):
        self.assertEqual(
            count_dnsmasq_queries(
                SAMPLE_LOG, "www.smoke.test", "A", from_ip="172.30.97.20"
            ),
            2,
        )

    def test_zero_when_absent(self):
        self.assertEqual(count_dnsmasq_queries(SAMPLE_LOG, "gone.example.test", "A"), 0)


if __name__ == "__main__":
    unittest.main()
