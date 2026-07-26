"""Lab companion process helpers (dnstap tracer; OTLP tracer later)."""

from __future__ import annotations

import os
import signal
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path


DNSTAP_SOCK_DEFAULT = Path("/tmp/conduit-perf-dnstap.sock")


@dataclass
class CompanionProcess:
    path: Path
    proc: subprocess.Popen[bytes]
    kind: str

    def stop(self, *, wait_s: float = 10.0) -> None:
        if self.proc.poll() is not None:
            return
        self.proc.send_signal(signal.SIGTERM)
        try:
            self.proc.wait(timeout=wait_s)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=5)


def sibling_binary(conduit: Path, name: str) -> Path | None:
    """Prefer a companion binary next to the Conduit binary."""
    candidate = conduit.expanduser().resolve().parent / name
    if candidate.is_file() and os.access(candidate, os.X_OK):
        return candidate
    return None


def resolve_dnstap_tracer(
    explicit: Path | None,
    *,
    conduit: Path,
) -> Path | None:
    if explicit is not None:
        return explicit if explicit.is_file() else None
    return sibling_binary(conduit, "conduit-dnstap-tracer")


def resolve_otlp_tracer(
    explicit: Path | None,
    *,
    conduit: Path,
) -> Path | None:
    if explicit is not None:
        return explicit if explicit.is_file() else None
    return sibling_binary(conduit, "conduit-otlp-metrics-tracer")


def resolve_conduitctl(
    explicit: Path | None,
    *,
    conduit: Path,
) -> Path | None:
    if explicit is not None:
        return explicit if explicit.is_file() else None
    return sibling_binary(conduit, "conduitctl")


def start_dnstap_tracer(
    binary: Path,
    *,
    sock: Path = DNSTAP_SOCK_DEFAULT,
    ready_timeout_s: float = 10.0,
) -> CompanionProcess:
    if not binary.is_file():
        raise FileNotFoundError(f"dnstap tracer not found: {binary}")
    if sock.exists():
        try:
            sock.unlink()
        except OSError:
            pass

    proc = subprocess.Popen(
        [str(binary), "-u", str(sock), "-f", "log"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    companion = CompanionProcess(path=binary, proc=proc, kind="dnstap_tracer")
    deadline = time.monotonic() + ready_timeout_s
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(
                f"conduit-dnstap-tracer exited early (code={proc.returncode})"
            )
        if sock.exists():
            # Brief settle so the listen accept loop is up.
            time.sleep(0.05)
            return companion
        time.sleep(0.05)
    companion.stop()
    raise TimeoutError(f"dnstap tracer socket not ready: {sock}")
