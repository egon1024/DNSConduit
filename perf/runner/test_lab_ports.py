"""Unit tests for shared lab/perf port conflict detection."""

from __future__ import annotations

import os
import socket
import unittest

from perf.runner.lab_ports import (
    ipv4_port_proc_key,
    pids_holding_tcp,
    pids_holding_udp,
    refuse_if_lab_ports_busy,
)


class LabPortsTests(unittest.TestCase):
    def test_ipv4_port_proc_key_matches_linux_proc_net(self):
        # 127.0.0.1:53 → classic /proc/net example 0100007F:0035
        self.assertEqual(ipv4_port_proc_key("127.0.0.1", 53), "0100007F:0035")
        self.assertEqual(ipv4_port_proc_key("127.0.2.1", 15353), "0102007F:3BF9")
        self.assertEqual(ipv4_port_proc_key("127.0.2.1", 19090), "0102007F:4A92")

    def test_pids_holding_udp_sees_bound_socket(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.addCleanup(sock.close)
        sock.bind(("127.0.2.1", 0))
        host, port = sock.getsockname()[:2]
        holders = pids_holding_udp(host, port)
        self.assertIn(os.getpid(), holders)

    def test_pids_holding_tcp_sees_listen_socket(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.addCleanup(sock.close)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("127.0.2.1", 0))
        sock.listen(1)
        host, port = sock.getsockname()[:2]
        holders = pids_holding_tcp(host, port)
        self.assertIn(os.getpid(), holders)

    def test_refuse_if_lab_ports_busy_when_dns_held(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.addCleanup(sock.close)
        # Bind the canonical DNS lab port when free; skip if something else holds it.
        try:
            sock.bind(("127.0.2.1", 15353))
        except OSError:
            self.skipTest("127.0.2.1:15353 already in use")
        msg = refuse_if_lab_ports_busy()
        self.assertIsNotNone(msg)
        assert msg is not None
        self.assertIn("15353", msg)
        self.assertIn("leftover manual-lab", msg)


if __name__ == "__main__":
    unittest.main()
