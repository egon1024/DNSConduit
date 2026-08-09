"""Detect foreign holders of the shared lab/perf loopback ports.

Manual labs and the performance harness intentionally share 127.0.2.1:15353
(DNS), :19090 (Prometheus), and :5199 (control). A leftover lab Conduit will
answer the harness readiness probe and metrics scrape, silently measuring the
wrong process. These helpers refuse that class of corruption.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

# Shared with manual-testing.md / perf fixtures.
DEFAULT_LAB_HOST = "127.0.2.1"
DEFAULT_DNS_PORT = 15353
DEFAULT_METRICS_PORT = 19090
DEFAULT_CONTROL_PORT = 5199

_SOCK_INODE = re.compile(r"^socket:\[(\d+)\]$")


def ipv4_port_proc_key(host: str, port: int) -> str:
    """Format ``local_address`` as in ``/proc/net/{tcp,udp}`` (IPv4)."""
    parts = [int(x) for x in host.split(".")]
    if len(parts) != 4 or any(p < 0 or p > 255 for p in parts):
        raise ValueError(f"expected IPv4 host, got {host!r}")
    if not (0 <= port <= 65535):
        raise ValueError(f"port out of range: {port}")
    # /proc/net uses little-endian host words on Linux.
    word = parts[0] | (parts[1] << 8) | (parts[2] << 16) | (parts[3] << 24)
    return f"{word:08X}:{port:04X}"


def _inodes_for_local(host: str, port: int, *, proto: str) -> set[str]:
    """Return socket inodes bound to ``host:port`` for ``tcp`` or ``udp``."""
    key = ipv4_port_proc_key(host, port)
    path = Path(f"/proc/net/{proto}")
    if not path.is_file():
        return set()
    inodes: set[str] = set()
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines()[1:]:
        cols = line.split()
        if len(cols) < 10:
            continue
        if cols[1].upper() == key:
            inodes.add(cols[9])
    return inodes


def _pids_for_inodes(inodes: set[str]) -> set[int]:
    if not inodes:
        return set()
    pids: set[int] = set()
    for ent in Path("/proc").iterdir():
        if not ent.name.isdigit():
            continue
        fd_dir = ent / "fd"
        try:
            for fd in fd_dir.iterdir():
                try:
                    target = os.readlink(fd)
                except OSError:
                    continue
                match = _SOCK_INODE.match(target)
                if match and match.group(1) in inodes:
                    pids.add(int(ent.name))
                    break
        except OSError:
            continue
    return pids


def pids_holding_udp(host: str, port: int) -> set[int]:
    """PIDs with a UDP socket bound to ``host:port``."""
    return _pids_for_inodes(_inodes_for_local(host, port, proto="udp"))


def pids_holding_tcp(host: str, port: int) -> set[int]:
    """PIDs with a TCP socket bound to ``host:port`` (LISTEN or otherwise)."""
    return _pids_for_inodes(_inodes_for_local(host, port, proto="tcp"))


def cmdline_for_pid(pid: int, *, max_len: int = 160) -> str:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return "(unknown)"
    text = raw.replace(b"\x00", b" ").decode("utf-8", errors="replace").strip()
    if not text:
        return "(unknown)"
    return text if len(text) <= max_len else text[: max_len - 3] + "..."


def describe_holders(host: str, port: int, *, proto: str) -> list[str]:
    """Human-readable ``pid …: cmdline`` lines for holders of a port."""
    if proto == "udp":
        pids = pids_holding_udp(host, port)
    elif proto == "tcp":
        pids = pids_holding_tcp(host, port)
    else:
        raise ValueError(f"unsupported proto {proto!r}")
    return [f"pid {pid}: {cmdline_for_pid(pid)}" for pid in sorted(pids)]


def refuse_if_lab_ports_busy(
    *,
    host: str = DEFAULT_LAB_HOST,
    dns_port: int = DEFAULT_DNS_PORT,
    metrics_port: int = DEFAULT_METRICS_PORT,
    control_port: int = DEFAULT_CONTROL_PORT,
) -> str | None:
    """Return an error message if shared lab/perf ports are already taken."""
    conflicts: list[str] = []
    for label, port, proto in (
        ("DNS UDP", dns_port, "udp"),
        ("Prometheus TCP", metrics_port, "tcp"),
        ("control TCP", control_port, "tcp"),
    ):
        holders = describe_holders(host, port, proto=proto)
        if holders:
            conflicts.append(f"{host}:{port} ({label}):")
            conflicts.extend(f"  {line}" for line in holders)
    if not conflicts:
        return None
    lines = [
        "refusing to measure: shared lab/perf ports are already in use. "
        "A leftover manual-lab Conduit will answer the readiness probe and "
        "metrics scrape, so every scenario silently measures that process "
        "(identical QPS / ~100% cache hit rate is the usual fingerprint).",
        *conflicts,
        "Stop the lab Conduit (Ctrl+C in its terminal, or kill the pid) "
        "and re-run.",
    ]
    return "\n".join(lines)
