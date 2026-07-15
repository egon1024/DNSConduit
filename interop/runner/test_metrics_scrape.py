"""Unit tests for Prometheus scrape text parsing (cache metrics oracle)."""

from __future__ import annotations

import unittest

from interop.runner.metrics_scrape import MetricSamples, parse_prom_text, sum_matching


SAMPLE = """\
# HELP conduit_cache_lookups_total Cache lookups
# TYPE conduit_cache_lookups_total counter
conduit_cache_lookups_total{cache="global",profile="default",result="miss"} 3
conduit_cache_lookups_total{cache="global",profile="default",result="hit"} 1
conduit_responses_total{listener="",protocol="udp",rcode="NOERROR",answer_source="forward"} 2
conduit_responses_total{listener="",protocol="udp",rcode="NOERROR",answer_source="cache"} 1
conduit_responses_total{listener="",protocol="udp",rcode="NXDOMAIN",answer_source="cache"} 1
"""


class ParsePromTextTests(unittest.TestCase):
    def test_sum_matching_by_result_label(self):
        samples = parse_prom_text(SAMPLE)
        self.assertEqual(sum_matching(samples, "conduit_cache_lookups_total", {"result": "miss"}), 3.0)
        self.assertEqual(sum_matching(samples, "conduit_cache_lookups_total", {"result": "hit"}), 1.0)

    def test_sum_matching_partial_labels(self):
        samples = parse_prom_text(SAMPLE)
        self.assertEqual(
            sum_matching(samples, "conduit_responses_total", {"answer_source": "cache"}),
            2.0,
        )

    def test_metric_samples_delta(self):
        before = MetricSamples.from_text(SAMPLE)
        after_text = SAMPLE.replace('result="hit"} 1', 'result="hit"} 4')
        after = MetricSamples.from_text(after_text)
        self.assertEqual(
            after.sum("conduit_cache_lookups_total", {"result": "hit"})
            - before.sum("conduit_cache_lookups_total", {"result": "hit"}),
            3.0,
        )

    def test_missing_series_is_zero(self):
        samples = parse_prom_text(SAMPLE)
        self.assertEqual(sum_matching(samples, "conduit_cache_lookups_total", {"result": "bypass"}), 0.0)


if __name__ == "__main__":
    unittest.main()
