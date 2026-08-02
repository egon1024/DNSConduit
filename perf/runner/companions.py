"""Lab companion process helpers (dnstap tracer, OTLP metrics tracer, scrape hammer)."""

from __future__ import annotations

import json
import os
import signal
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

from .cpuaffinity import taskset_prefix
from .procs import die_with_parent, register_child, unregister_child

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
        try:
            if self.proc.poll() is not None:
                return
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=wait_s)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)
        finally:
            if self.proc.pid is not None:
                unregister_child(self.proc.pid)


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
    cpuset: str | None = None,
) -> CompanionProcess:
    if not binary.is_file():
        raise FileNotFoundError(f"dnstap tracer not found: {binary}")
    if sock.exists():
        try:
            sock.unlink()
        except OSError:
            pass

    proc = subprocess.Popen(
        [*taskset_prefix(cpuset), str(binary), "-u", str(sock), "-f", "log"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        preexec_fn=die_with_parent,
    )
    if proc.pid is not None:
        register_child(proc.pid, kind="dnstap_tracer")
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
    cpuset: str | None = None,
) -> CompanionProcess:
    if not binary.is_file():
        raise FileNotFoundError(f"OTLP metrics tracer not found: {binary}")

    cmd = [*taskset_prefix(cpuset), str(binary), "-a", listen, "-p", path, "-f", "log"]
    if delay_ms > 0:
        cmd.extend(["--delay-ms", str(delay_ms)])

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        preexec_fn=die_with_parent,
    )
    if proc.pid is not None:
        register_child(proc.pid, kind="otlp_tracer")
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


PROMETHEUS_SCRAPE_DEFAULT = "http://127.0.2.1:19090/metrics"


@dataclass
class ScrapeHammer:
    """Background HTTP GET loop against the Prometheus scrape path during load."""

    url: str
    interval_ms: int
    _stop: threading.Event = field(default_factory=threading.Event, repr=False)
    _thread: threading.Thread | None = field(default=None, repr=False)
    kind: str = "scrape_hammer"
    scrape_ok: int = 0
    scrape_fail: int = 0

    def start(self) -> None:
        if self._thread is not None:
            return
        self._stop.clear()

        def _loop() -> None:
            interval_s = max(self.interval_ms, 1) / 1000.0
            while not self._stop.is_set():
                try:
                    with urllib.request.urlopen(self.url, timeout=2.0) as resp:
                        resp.read()
                    self.scrape_ok += 1
                except (urllib.error.URLError, TimeoutError, OSError):
                    self.scrape_fail += 1
                self._stop.wait(interval_s)

        self._thread = threading.Thread(
            target=_loop, name="perf-scrape-hammer", daemon=True
        )
        self._thread.start()

    def stop(self, *, wait_s: float = 5.0) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=wait_s)
            self._thread = None

    def stats(self) -> dict[str, int]:
        return {
            "scrape_hammer_ok": self.scrape_ok,
            "scrape_hammer_fail": self.scrape_fail,
        }


def start_scrape_hammer(
    *,
    url: str = PROMETHEUS_SCRAPE_DEFAULT,
    interval_ms: int = 100,
) -> ScrapeHammer:
    hammer = ScrapeHammer(url=url, interval_ms=interval_ms)
    hammer.start()
    return hammer
