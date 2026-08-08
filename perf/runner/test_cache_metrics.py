"""Unit tests for Prometheus cache lookup scrape helpers."""

from __future__ import annotations

import unittest
from unittest import mock

from perf.runner.companions import scrape_cache_lookup_counts


class ScrapeCacheLookupCountsTests(unittest.TestCase):
    def test_sums_hit_and_miss_and_derives_rate(self):
        body = (
            "# HELP conduit_cache_lookups_total\n"
            'conduit_cache_lookups_total{cache="global",profile="default",result="hit"} 75\n'
            'conduit_cache_lookups_total{cache="global",profile="default",result="miss"} 25\n'
            'conduit_cache_lookups_total{cache="global",profile="default",result="bypass"} 3\n'
        )
        with mock.patch(
            "perf.runner.companions.urllib.request.urlopen"
        ) as urlopen:
            resp = mock.MagicMock()
            resp.read.return_value = body.encode("utf-8")
            resp.__enter__.return_value = resp
            resp.__exit__.return_value = False
            urlopen.return_value = resp
            stats = scrape_cache_lookup_counts("http://127.0.2.1:19090/metrics")
        self.assertEqual(stats["cache_lookups_hit"], 75)
        self.assertEqual(stats["cache_lookups_miss"], 25)
        self.assertEqual(stats["cache_hit_rate"], 75.0)

    def test_empty_totals_yield_zero_rate(self):
        with mock.patch(
            "perf.runner.companions.urllib.request.urlopen"
        ) as urlopen:
            resp = mock.MagicMock()
            resp.read.return_value = b"# no series\n"
            resp.__enter__.return_value = resp
            resp.__exit__.return_value = False
            urlopen.return_value = resp
            stats = scrape_cache_lookup_counts("http://example/metrics")
        self.assertEqual(stats["cache_lookups_hit"], 0)
        self.assertEqual(stats["cache_lookups_miss"], 0)
        self.assertEqual(stats["cache_hit_rate"], 0.0)


if __name__ == "__main__":
    unittest.main()
