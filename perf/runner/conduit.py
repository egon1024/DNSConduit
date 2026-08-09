"""Conduit process lifecycle for binary-driven performance runs."""

from __future__ import annotations

import io
import os
import signal
import socket
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

from .cpuaffinity import taskset_prefix
from .lab_ports import cmdline_for_pid, pids_holding_udp
from .procs import die_with_parent, register_child, unregister_child


@dataclass
class ConduitProcess:
    path: Path
    config: Path
    proc: subprocess.Popen[bytes]
    listen_host: str
    listen_port: int
    log_file: io.BufferedRandom | None = None

    @property
    def pid(self) -> int | None:
        return self.proc.pid

    def poll(self) -> int | None:
        return self.proc.poll()

    def stop(self, *, sig: signal.Signals = signal.SIGTERM, wait_s: float = 30.0) -> float:
        """Signal Conduit and wait for exit. Returns wall seconds until exit."""
        try:
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
        finally:
            if self.proc.pid is not None:
                unregister_child(self.proc.pid)
            if self.log_file is not None:
                try:
                    self.log_file.close()
                except Exception:
                    pass

    def tail_log(self, max_bytes: int = 4000) -> str:
        """Best-effort read of captured stdout/stderr for diagnostics."""
        if self.log_file is None or self.log_file.closed:
            return ""
        try:
            self.log_file.seek(0)
            data = self.log_file.read()
        except Exception:
            return ""
        return data[-max_bytes:].decode(errors="replace")


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
    cpuset: str | None = None,
) -> ConduitProcess:
    if not binary.is_file():
        raise FileNotFoundError(f"conduit binary not found: {binary}")
    if not config.is_file():
        raise FileNotFoundError(f"conduit config not found: {config}")

    # A foreign Conduit (manual lab left running) answers DNS on these ports
    # and would make readiness succeed before *our* child binds — every
    # subsequent dnsperf/scrape then measures the lab process. Refuse up front.
    prior = pids_holding_udp(listen_host, listen_port)
    if prior:
        holders = "; ".join(
            f"pid {pid} ({cmdline_for_pid(pid)})" for pid in sorted(prior)
        )
        raise RuntimeError(
            f"UDP {listen_host}:{listen_port} already held ({holders}); "
            "stop the leftover process before starting a perf Conduit"
        )

    # Isolate the measured variable: do not let the harness's own default or
    # the operator's shell RUST_LOG override the scenario's `logging.level`.
    # Conduit's env_filter_from_config lets RUST_LOG win when set, so any
    # ambient value here would silently defeat a logging-level comparison.
    run_env = os.environ.copy()
    run_env.pop("RUST_LOG", None)
    if env:
        run_env.update(env)

    # Capture stdout/stderr to a seekable temp file instead of an unread
    # subprocess.PIPE. A pipe fills its OS buffer (~64KB) if nothing drains
    # it; under high query volume or verbose logging.level that would make
    # Conduit block on its own log writes mid-run, corrupting the very
    # throughput/latency numbers the scenario is trying to measure.
    log_file = tempfile.TemporaryFile(prefix="conduit-perf-", suffix=".log")

    argv = [*taskset_prefix(cpuset), str(binary), str(config), *extra_args]
    proc = subprocess.Popen(
        argv,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        env=run_env,
        preexec_fn=die_with_parent,
    )
    if proc.pid is not None:
        register_child(proc.pid, kind="conduit")
    cp = ConduitProcess(
        path=binary,
        config=config,
        proc=proc,
        listen_host=listen_host,
        listen_port=listen_port,
        log_file=log_file,
    )
    deadline = time.monotonic() + ready_timeout_s
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            tail = cp.tail_log(2000)
            log_file.close()
            raise RuntimeError(
                f"conduit exited early (code={proc.returncode}): {tail}"
            )
        holders = pids_holding_udp(listen_host, listen_port)
        if proc.pid is not None and proc.pid in holders:
            # Require our child to own the socket — not merely that *someone*
            # answers DNS (a race with a lab process that grabbed the port).
            strangers = holders - {proc.pid}
            if strangers:
                proc.kill()
                log_file.close()
                detail = "; ".join(
                    f"pid {pid} ({cmdline_for_pid(pid)})"
                    for pid in sorted(strangers)
                )
                raise RuntimeError(
                    f"UDP {listen_host}:{listen_port} is shared with foreign "
                    f"holders ({detail}); refusing ambiguous readiness"
                )
            if probe_dns_answer(listen_host, listen_port, timeout_s=0.3):
                return cp
        time.sleep(0.1)
    proc.kill()
    log_file.close()
    raise TimeoutError(
        f"conduit did not become ready on {listen_host}:{listen_port} "
        f"within {ready_timeout_s}s"
    )
