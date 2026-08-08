"""Unit tests for the stub upstream responders."""

from __future__ import annotations

import os
import socket
import struct
import time
import unittest

from perf.runner.cpuaffinity import parse_cpuset
from perf.runner.upstream import (
    DEFAULT_FAST_WORKERS,
    _build_a_response,
    start_fast_upstream,
    start_slow_upstream,
)

HOST = "127.0.0.1"
PORT = 15399
QUERY = (
    b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00"
    b"\x03www\x04perf\x04test\x00\x00\x01\x00\x01"
)


def _ask(timeout_s: float = 2.0) -> bytes:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(timeout_s)
        sock.sendto(QUERY, (HOST, PORT))
        data, _peer = sock.recvfrom(2048)
        return data


class BuildResponseTests(unittest.TestCase):
    def test_answer_echoes_question_and_sets_qr(self):
        resp = _build_a_response(QUERY)
        self.assertEqual(resp[:2], QUERY[:2])
        self.assertEqual(resp[2] & 0x80, 0x80)
        self.assertEqual(resp[6:8], b"\x00\x01")
        self.assertTrue(resp.endswith(socket.inet_aton("192.0.2.10")))

    def test_custom_ttl_is_encoded(self):
        resp = _build_a_response(QUERY, ttl=2)
        # ANCOUNT=1 answer starts after question; TTL is bytes at offset after
        # compression pointer (2) + type/class (4) = 6 bytes into answer RDATA header.
        # Locate the A rdata TTL: last 10 bytes are type(2)+class(2)+ttl(4)+rdlen(2) before rdata.
        self.assertEqual(struct.unpack("!I", resp[-10:-6])[0], 2)


class FastUpstreamTests(unittest.TestCase):
    def test_pool_answers_queries(self):
        stub = start_fast_upstream(host=HOST, port=PORT, workers=2)
        try:
            self.assertEqual(len(stub._pids), 2)
            for _ in range(5):
                self.assertEqual(_ask()[:2], QUERY[:2])
        finally:
            stub.stop()
        self.assertEqual(stub._pids, [])

    def test_default_pool_has_several_workers(self):
        self.assertGreater(DEFAULT_FAST_WORKERS, 1)

    def test_workers_honor_requested_cpuset(self):
        allowed = sorted(os.sched_getaffinity(0))
        if len(allowed) < 2:
            self.skipTest("needs at least two usable CPUs")
        target = str(allowed[-1])
        stub = start_fast_upstream(host=HOST, port=PORT, workers=1, cpuset=target)
        try:
            self.assertEqual(_ask()[:2], QUERY[:2])
            self.assertEqual(os.sched_getaffinity(stub._pids[0]), {allowed[-1]})
        finally:
            stub.stop()

    def test_unpinned_workers_keep_full_affinity(self):
        stub = start_fast_upstream(host=HOST, port=PORT, workers=1)
        try:
            self.assertEqual(_ask()[:2], QUERY[:2])
            self.assertEqual(
                os.sched_getaffinity(stub._pids[0]), os.sched_getaffinity(0)
            )
        finally:
            stub.stop()


class CpusetParsingTests(unittest.TestCase):
    def test_ranges_and_singletons(self):
        self.assertEqual(parse_cpuset("0-3,8"), {0, 1, 2, 3, 8})

    def test_empty_and_malformed_mean_do_not_pin(self):
        self.assertEqual(parse_cpuset(None), set())
        self.assertEqual(parse_cpuset(""), set())
        self.assertEqual(parse_cpuset("0-x"), set())


class SlowUpstreamTests(unittest.TestCase):
    def test_delay_is_applied(self):
        stub = start_slow_upstream(host=HOST, port=PORT, delay_ms=100.0, workers=1)
        try:
            started = time.monotonic()
            _ask()
            self.assertGreaterEqual(time.monotonic() - started, 0.08)
        finally:
            stub.stop()

    def test_one_worker_overlaps_delayed_replies(self):
        """A delayed backend must stay concurrent, not serialize one query per delay."""
        stub = start_slow_upstream(host=HOST, port=PORT, delay_ms=100.0, workers=1)
        try:
            sockets = [
                socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for _ in range(10)
            ]
            started = time.monotonic()
            for sock in sockets:
                sock.settimeout(3.0)
                sock.sendto(QUERY, (HOST, PORT))
            for sock in sockets:
                sock.recvfrom(2048)
            elapsed = time.monotonic() - started
            for sock in sockets:
                sock.close()
            # Serialized sleeps would need ~1.0s for ten queries at 100 ms each.
            self.assertLess(elapsed, 0.6)
        finally:
            stub.stop()


if __name__ == "__main__":
    unittest.main()
