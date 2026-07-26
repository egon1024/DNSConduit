"""Lab companion process helpers (dnstap tracer, OTLP metrics tracer)."""

from __future__ import annotations

import json
import os
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path


DNSTAP_SOCK_DEFAULT = Path("/tmp/conduit-perf-dnstap.sock")
OTLP_LISTEN_DEFAULT = "127.0.2.1:4318"
OTLP_STATS_URL_DEFAULT = f"http://{OTLP_LISTEN_DEFAULT}/stats"


@dataclass
class CompanionProcess:
    path: Path
    proc: subprocess.Popen[bytes]
    kind: str
    listen: str | None = None

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


def _tcp_ready(host: str, port: int, *, timeout_s: float = 0.05) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout_s):
            return True
    except OSError:
        return False


def start_otlp_tracer(
    binary: Path,
    *,
    listen: str = OTLP_LISTEN_DEFAULT,
    path: str = "/v1/metrics",
    delay_ms: int = 0,
    ready_timeout_s: float = 10.0,
) -> CompanionProcess:
    if not binary.is_file():
        raise FileNotFoundError(f"OTLP metrics tracer not found: {binary}")

    cmd = [str(binary), "-a", listen, "-p", path, "-f", "log"]
    if delay_ms > 0:
        cmd.extend(["--delay-ms", str(delay_ms)])

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    companion = CompanionProcess(
        path=binary, proc=proc, kind="otlp_tracer", listen=listen
    )
    host, port_s = listen.rsplit(":", 1)
    port = int(port_s)
    deadline = time.monotonic() + ready_timeout_s
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(
                f"conduit-otlp-metrics-tracer exited early (code={proc.returncode})"
            )
        if _tcp_ready(host, port):
            time.sleep(0.05)
            return companion
        time.sleep(0.05)
    companion.stop()
    raise TimeoutError(f"OTLP metrics tracer not ready: {listen}")


def fetch_otlp_stats(
    listen: str = OTLP_LISTEN_DEFAULT,
    *,
    timeout_s: float = 2.0,
) -> dict[str, int] | None:
    """Return accept/failure counts from the tracer GET /stats endpoint."""
    url = f"http://{listen}/stats"
    try:
        with urllib.request.urlopen(url, timeout=timeout_s) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, OSError):
        return None
    accepts = payload.get("accepts")
    failures = payload.get("failures")
    if not isinstance(accepts, int) or not isinstance(failures, int):
        return None
    return {"otlp_accepts": accepts, "otlp_failures": failures}
