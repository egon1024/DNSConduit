"""Stub upstream DNS responders (fast and artificial-slow).

The stub must never be the binding constraint on a forward-path scenario: if it
is, achieved QPS measures the responder instead of Conduit, and Conduit starts
fast-failing queries it cannot forward (which a loadgen counts as completed
responses). Two properties keep the stub out of the way:

* **Capacity** — replies are served by several forked worker processes sharing
  one ``SO_REUSEPORT`` port, so the responder scales past a single CPU and past
  the CPython global interpreter lock.
* **Concurrency under delay** — the artificial-slow responder holds delayed
  replies in a per-worker timer heap instead of sleeping. A 50 ms upstream then
  models a high-latency backend that still accepts new queries, rather than a
  backend that serializes one query per delay interval.
"""

from __future__ import annotations

import heapq
import os
import selectors
import signal
import socket
import struct
import time
from dataclasses import dataclass, field

from .cpuaffinity import parse_cpuset
from .procs import die_with_parent, register_child, unregister_child

# Worker counts are sized for the maintainer lab; scenarios never tune them.
DEFAULT_FAST_WORKERS = 8
DEFAULT_SLOW_WORKERS = 4
RECV_BUFFER_BYTES = 8 << 20
_BATCH_PER_WAKEUP = 64


def _parse_qname(data: bytes, offset: int) -> tuple[str, int]:
    labels: list[str] = []
    while True:
        if offset >= len(data):
            raise ValueError("truncated qname")
        length = data[offset]
        offset += 1
        if length == 0:
            break
        if length & 0xC0:
            # compression pointer — not expected on queries we craft, but tolerate
            offset += 1
            break
        labels.append(data[offset : offset + length].decode("ascii", errors="replace"))
        offset += length
    return ".".join(labels) + ".", offset


def _build_a_response(query: bytes, addr: str = "192.0.2.10", ttl: int = 60) -> bytes:
    if len(query) < 12:
        raise ValueError("short query")
    # Copy header, set QR=1 RA=1, ancount=1
    header = bytearray(query[:12])
    header[2] = 0x81  # QR + RD echo-ish
    header[3] = 0x80  # RA
    header[6] = 0
    header[7] = 1  # ANCOUNT
    # Question section from query
    _qname, qend = _parse_qname(query, 12)
    question = query[12:qend] + query[qend : qend + 4]
    # Answer: pointer to qname at offset 12
    answer = b"\xc0\x0c" + b"\x00\x01\x00\x01" + struct.pack("!I", ttl)
    rdata = socket.inet_aton(addr)
    answer += struct.pack("!H", len(rdata)) + rdata
    return bytes(header) + question + answer


def _bind_worker_socket(host: str, port: int) -> socket.socket:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, RECV_BUFFER_BYTES)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, RECV_BUFFER_BYTES)
    except OSError:
        pass
    sock.bind((host, port))
    return sock


def _serve_forever(sock: socket.socket, *, delay_s: float, answer: str) -> None:
    """Reply loop for one worker process (never returns)."""
    sock.setblocking(False)
    recv = sock.recvfrom
    send = sock.sendto
    if delay_s <= 0:
        selector = selectors.DefaultSelector()
        selector.register(sock, selectors.EVENT_READ)
        while True:
            selector.select()
            for _ in range(_BATCH_PER_WAKEUP):
                try:
                    data, peer = recv(2048)
                except BlockingIOError:
                    break
                except OSError:
                    return
                try:
                    send(_build_a_response(data, addr=answer), peer)
                except (OSError, ValueError):
                    continue
        return

    # Delayed replies: queue by due time so in-flight queries overlap.
    pending: list[tuple[float, int, bytes, tuple[str, int]]] = []
    seq = 0
    selector = selectors.DefaultSelector()
    selector.register(sock, selectors.EVENT_READ)
    while True:
        timeout: float | None = None
        if pending:
            timeout = max(0.0, pending[0][0] - time.monotonic())
        selector.select(timeout)
        for _ in range(_BATCH_PER_WAKEUP):
            try:
                data, peer = recv(2048)
            except BlockingIOError:
                break
            except OSError:
                return
            try:
                resp = _build_a_response(data, addr=answer)
            except (ValueError, IndexError):
                continue
            seq += 1
            heapq.heappush(pending, (time.monotonic() + delay_s, seq, resp, peer))
        now = time.monotonic()
        while pending and pending[0][0] <= now:
            _due, _seq, resp, peer = heapq.heappop(pending)
            try:
                send(resp, peer)
            except OSError:
                continue


@dataclass
class StubUpstream:
    """Forked pool of UDP responders sharing one ``SO_REUSEPORT`` port."""

    host: str
    port: int
    delay_ms: float
    answer: str = "192.0.2.10"
    workers: int = DEFAULT_FAST_WORKERS
    cpuset: str | None = None
    _pids: list[int] = field(default_factory=list)
    _sockets: list[socket.socket] = field(default_factory=list)

    def start(self) -> None:
        if self._pids:
            return
        count = max(1, int(self.workers))
        delay_s = max(0.0, self.delay_ms) / 1000.0
        cpus = parse_cpuset(self.cpuset)
        # Bind in the parent so the port is ready the moment start() returns.
        self._sockets = [_bind_worker_socket(self.host, self.port) for _ in range(count)]
        for sock in self._sockets:
            pid = os.fork()
            if pid == 0:
                try:
                    die_with_parent()
                    for other in self._sockets:
                        if other is not sock:
                            other.close()
                    if cpus:
                        os.sched_setaffinity(0, cpus)
                    signal.signal(signal.SIGTERM, signal.SIG_DFL)
                    _serve_forever(sock, delay_s=delay_s, answer=self.answer)
                except BaseException:  # noqa: BLE001 — child must never unwind
                    pass
                finally:
                    os._exit(0)
            self._pids.append(pid)
            register_child(pid, kind="stub-upstream")

    def stop(self) -> None:
        for pid in self._pids:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            unregister_child(pid)
        for pid in self._pids:
            try:
                os.waitpid(pid, 0)
            except ChildProcessError:
                continue
        self._pids = []
        for sock in self._sockets:
            try:
                sock.close()
            except OSError:
                pass
        self._sockets = []


def start_fast_upstream(
    host: str = "127.0.2.1",
    port: int = 15300,
    workers: int = DEFAULT_FAST_WORKERS,
    cpuset: str | None = None,
) -> StubUpstream:
    stub = StubUpstream(
        host=host, port=port, delay_ms=0, workers=workers, cpuset=cpuset
    )
    stub.start()
    return stub


def start_slow_upstream(
    host: str = "127.0.2.1",
    port: int = 15300,
    delay_ms: float = 50.0,
    workers: int = DEFAULT_SLOW_WORKERS,
    cpuset: str | None = None,
) -> StubUpstream:
    stub = StubUpstream(
        host=host, port=port, delay_ms=delay_ms, workers=workers, cpuset=cpuset
    )
    stub.start()
    return stub
