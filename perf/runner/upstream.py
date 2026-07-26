"""Stub upstream DNS responders (fast and artificial-slow)."""

from __future__ import annotations

import socket
import struct
import threading
import time
from dataclasses import dataclass


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
    qname, qend = _parse_qname(query, 12)
    question = query[12:qend] + query[qend : qend + 4]
    # Answer: pointer to qname at offset 12
    answer = b"\xc0\x0c" + b"\x00\x01\x00\x01" + struct.pack("!I", ttl)
    rdata = socket.inet_aton(addr)
    answer += struct.pack("!H", len(rdata)) + rdata
    return bytes(header) + question + answer


@dataclass
class StubUpstream:
    host: str
    port: int
    delay_ms: float
    answer: str = "192.0.2.10"
    _thread: threading.Thread | None = None
    _sock: socket.socket | None = None
    _stop: threading.Event | None = None

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        stop = threading.Event()
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind((self.host, self.port))
        sock.settimeout(0.2)
        self._sock = sock
        self._stop = stop

        def _loop() -> None:
            while not stop.is_set():
                try:
                    data, addr = sock.recvfrom(4096)
                except socket.timeout:
                    continue
                except OSError:
                    break
                if self.delay_ms > 0:
                    time.sleep(self.delay_ms / 1000.0)
                try:
                    resp = _build_a_response(data, addr=self.answer)
                    sock.sendto(resp, addr)
                except Exception:
                    continue

        self._thread = threading.Thread(target=_loop, name="perf-stub-upstream", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        if self._stop:
            self._stop.set()
        if self._sock:
            try:
                self._sock.close()
            except OSError:
                pass
        if self._thread:
            self._thread.join(timeout=2.0)
        self._thread = None
        self._sock = None
        self._stop = None


def start_fast_upstream(host: str = "127.0.2.1", port: int = 15300) -> StubUpstream:
    stub = StubUpstream(host=host, port=port, delay_ms=0)
    stub.start()
    return stub


def start_slow_upstream(
    host: str = "127.0.2.1",
    port: int = 15300,
    delay_ms: float = 50.0,
) -> StubUpstream:
    stub = StubUpstream(host=host, port=port, delay_ms=delay_ms)
    stub.start()
    return stub
