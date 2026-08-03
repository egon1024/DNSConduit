"""UDP receive-buffer preflight for the performance harness.

Elevated dnsperf recipes can overflow Conduit's UDP socket receive queue when
the host ``net.core.rmem_max`` (and fixture ``listeners.rcvbuf``) stay at the
tiny OS default (~208 KiB). Dropped datagrams never reach Conduit; dnsperf
counts them as **Queries lost**, while Conduit metrics show queries ==
responses for everything that arrived. That looks like product unreliability
but is host socket buffering.

Publish-quality runs require ``rmem_max`` large enough to honor the fixture
``listeners.rcvbuf`` (4 MiB). Override with ``--allow-suboptimal-udp-buffers``
only for intentional noisy probes.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Literal

# Match perf fixture ``listeners.rcvbuf: 4194304``.
MIN_RMEM_MAX_BYTES = 4 << 20
_RMEM_MAX_PATH = Path("/proc/sys/net/core/rmem_max")
_RMEM_DEFAULT_PATH = Path("/proc/sys/net/core/rmem_default")


@dataclass(frozen=True)
class UdpBufferCheck:
    status: Literal["ok", "suboptimal", "unavailable"]
    rmem_max: int | None
    rmem_default: int | None
    message: str = ""


def _read_sysctl(path: Path) -> int | None:
    try:
        return int(path.read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return None


def check_host_udp_buffers(
    *,
    min_rmem_max: int = MIN_RMEM_MAX_BYTES,
) -> UdpBufferCheck:
    rmem_max = _read_sysctl(_RMEM_MAX_PATH)
    rmem_default = _read_sysctl(_RMEM_DEFAULT_PATH)
    if rmem_max is None:
        return UdpBufferCheck(
            status="unavailable",
            rmem_max=None,
            rmem_default=rmem_default,
            message="net.core.rmem_max not readable; skipping UDP buffer check",
        )
    if rmem_max < min_rmem_max:
        return UdpBufferCheck(
            status="suboptimal",
            rmem_max=rmem_max,
            rmem_default=rmem_default,
            message=(
                f"net.core.rmem_max={rmem_max} is below {min_rmem_max} "
                f"(fixture listeners.rcvbuf). Elevated dnsperf will drop "
                f"ingress UDP (kernel RcvbufErrors) and report Queries lost "
                f"even though Conduit answers every datagram it receives.\n\n"
                f"Raise socket memory limits, then re-run:\n"
                f"  sudo sysctl -w net.core.rmem_max=16777216 "
                f"net.core.rmem_default=4194304\n"
                f"  # optional persist: /etc/sysctl.d/99-conduit-perf.conf\n\n"
                f"Or continue with a noisy probe:\n"
                f"  python3 -m perf.runner run … --allow-suboptimal-udp-buffers"
            ),
        )
    return UdpBufferCheck(
        status="ok",
        rmem_max=rmem_max,
        rmem_default=rmem_default,
    )


def require_udp_buffers_ok(
    check: UdpBufferCheck,
    *,
    allow_suboptimal: bool,
) -> str | None:
    """Return an error message to print+exit, or None to continue."""
    if check.status == "suboptimal" and not allow_suboptimal:
        return check.message
    return None
