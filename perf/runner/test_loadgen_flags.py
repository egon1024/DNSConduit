"""Unit tests for dnsperf flag construction."""

from __future__ import annotations

import unittest
from pathlib import Path

from perf.runner.loadgen import _dnsperf_flags


class DnsperfFlagsTest(unittest.TestCase):
    def test_default_omits_max_outstanding(self) -> None:
        flags = _dnsperf_flags(
            server="127.0.2.1",
            port=15353,
            query_file=Path("perf-a.txt"),
            clients=4,
            threads=2,
            limit_qps=None,
            max_outstanding=None,
            time_s=10,
            mode="native",
            extra_flags=(),
        )
        self.assertEqual(
            flags,
            [
                "-s",
                "127.0.2.1",
                "-p",
                "15353",
                "-d",
                "perf-a.txt",
                "-c",
                "4",
                "-T",
                "2",
                "-l",
                "10",
            ],
        )
        self.assertNotIn("-q", flags)

    def test_elevated_clients_threads_and_outstanding(self) -> None:
        flags = _dnsperf_flags(
            server="127.0.2.1",
            port=15353,
            query_file=Path("perf-a.txt"),
            clients=16,
            threads=8,
            limit_qps=None,
            max_outstanding=2000,
            time_s=8,
            mode="docker",
            extra_flags=(),
        )
        self.assertEqual(flags[flags.index("-c") + 1], "16")
        self.assertEqual(flags[flags.index("-T") + 1], "8")
        self.assertEqual(flags[flags.index("-q") + 1], "2000")
        self.assertEqual(flags[flags.index("-d") + 1], "/queries/perf-a.txt")

    def test_docker_uses_query_file_basename(self) -> None:
        flags = _dnsperf_flags(
            server="127.0.2.1",
            port=15353,
            query_file=Path("perf/fixtures/queries/perf-churn-a.txt"),
            clients=4,
            threads=2,
            limit_qps=None,
            max_outstanding=None,
            time_s=5,
            mode="docker",
            extra_flags=(),
        )
        self.assertEqual(flags[flags.index("-d") + 1], "/queries/perf-churn-a.txt")


if __name__ == "__main__":
    unittest.main()
