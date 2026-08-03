"""Unit tests for UDP rmem_max preflight."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from perf.runner.udpbuffers import (
    MIN_RMEM_MAX_BYTES,
    UdpBufferCheck,
    check_host_udp_buffers,
    require_udp_buffers_ok,
)


class UdpBufferTests(unittest.TestCase):
    def test_ok_when_rmem_max_meets_floor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rmax = root / "rmem_max"
            rdef = root / "rmem_default"
            rmax.write_text(str(MIN_RMEM_MAX_BYTES) + "\n", encoding="utf-8")
            rdef.write_text(str(MIN_RMEM_MAX_BYTES) + "\n", encoding="utf-8")
            with (
                mock.patch("perf.runner.udpbuffers._RMEM_MAX_PATH", rmax),
                mock.patch("perf.runner.udpbuffers._RMEM_DEFAULT_PATH", rdef),
            ):
                check = check_host_udp_buffers()
            self.assertEqual(check.status, "ok")
            self.assertIsNone(require_udp_buffers_ok(check, allow_suboptimal=False))

    def test_suboptimal_when_rmem_max_too_small(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rmax = root / "rmem_max"
            rdef = root / "rmem_default"
            rmax.write_text("212992\n", encoding="utf-8")
            rdef.write_text("212992\n", encoding="utf-8")
            with (
                mock.patch("perf.runner.udpbuffers._RMEM_MAX_PATH", rmax),
                mock.patch("perf.runner.udpbuffers._RMEM_DEFAULT_PATH", rdef),
            ):
                check = check_host_udp_buffers()
            self.assertEqual(check.status, "suboptimal")
            err = require_udp_buffers_ok(check, allow_suboptimal=False)
            self.assertIsNotNone(err)
            self.assertIn("rmem_max", err or "")
            self.assertIsNone(require_udp_buffers_ok(check, allow_suboptimal=True))

    def test_unavailable_when_sysctl_missing(self) -> None:
        missing = Path("/tmp/conduit-perf-missing-rmem-max-does-not-exist")
        with mock.patch("perf.runner.udpbuffers._RMEM_MAX_PATH", missing):
            check = check_host_udp_buffers()
        self.assertEqual(check.status, "unavailable")
        self.assertIsNone(require_udp_buffers_ok(check, allow_suboptimal=False))


if __name__ == "__main__":
    unittest.main()
