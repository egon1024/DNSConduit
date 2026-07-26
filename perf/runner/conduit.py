"""Conduit process lifecycle for binary-driven performance runs."""

from __future__ import annotations

import os
import signal
import socket
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


@dataclass
class ConduitProcess:
    path: Path
    config: Path
    proc: subprocess.Popen[bytes]
    listen_host: str
    listen_port: int

    @property
    def pid(self) -> int | None:
        return self.proc.pid

    def poll(self) -> int | None:
        return self.proc.poll()

    def stop(self, *, sig: signal.Signals = signal.SIGTERM, wait_s: float = 30.0) -> float:
        """Signal Conduit and wait for exit. Returns wall seconds until exit."""
        if self.proc.poll() is not None:
            return 0.0
        started = time.monotonic()
        self.proc.send_signal(sig)
        try:
            self.proc.wait(timeout=wait_s)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=5)
        return time.monotonic() - started


def _udp_ready(host: str, port: int, timeout_s: float = 0.2) -> bool:
    """Best-effort readiness: socket is bound (does not prove DNS answers yet)."""
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(timeout_s)
            # Empty probe; we only care that something accepts on the port.
            sock.sendto(b"\x00", (host, port))
            return True
    except OSError:
        return False


def probe_dns_answer(
    host: str,
    port: int,
    qname: str = "www.perf.test.",
    timeout_s: float = 1.0,
) -> bool:
    """Send a minimal DNS A query; return True if any UDP response arrives."""
    # Minimal DNS header + QNAME + QTYPE A + QCLASS IN (no EDNS).
    labels = [lab.encode("ascii") for lab in qname.rstrip(".").split(".") if lab]
    qname_wire = b"".join(bytes([len(lab)]) + lab for lab in labels) + b"\x00"
    header = b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00"
    packet = header + qname_wire + b"\x00\x01\x00\x01"
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(timeout_s)
            sock.sendto(packet, (host, port))
            data, _ = sock.recvfrom(4096)
            return len(data) >= 12
    except OSError:
        return False


def conduit_version(path: Path) -> str:
    try:
        out = subprocess.check_output(
            [str(path), "--version"],
            stderr=subprocess.STDOUT,
            timeout=10,
            text=True,
        )
        return out.strip().splitlines()[0] if out.strip() else "unknown"
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def start_conduit(
    binary: Path,
    config: Path,
    *,
    listen_host: str = "127.0.2.1",
    listen_port: int = 15353,
    env: dict[str, str] | None = None,
    ready_timeout_s: float = 30.0,
    extra_args: Sequence[str] = (),
) -> ConduitProcess:
    if not binary.is_file():
        raise FileNotFoundError(f"conduit binary not found: {binary}")
    if not config.is_file():
        raise FileNotFoundError(f"conduit config not found: {config}")

    run_env = os.environ.copy()
    run_env.setdefault("RUST_LOG", "info")
    if env:
        run_env.update(env)

    proc = subprocess.Popen(
        [str(binary), str(config), *extra_args],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=run_env,
    )
    cp = ConduitProcess(
        path=binary,
        config=config,
        proc=proc,
        listen_host=listen_host,
        listen_port=listen_port,
    )
    deadline = time.monotonic() + ready_timeout_s
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            out = b""
            if proc.stdout:
                out = proc.stdout.read() or b""
            raise RuntimeError(
                f"conduit exited early (code={proc.returncode}): "
                f"{out.decode(errors='replace')[:2000]}"
            )
        if probe_dns_answer(listen_host, listen_port, timeout_s=0.3):
            return cp
        time.sleep(0.1)
    proc.kill()
    raise TimeoutError(
        f"conduit did not become ready on {listen_host}:{listen_port} "
        f"within {ready_timeout_s}s"
    )
