#!/usr/bin/env python3
"""UDP DNS mock: answer every query with RCODE SERVFAIL (2).

For cache-policy lab manual section 2 (SERVFAIL storage). Listens on 127.0.2.1:15300
by default. Ctrl-C to stop.
"""
from __future__ import annotations

import socket
import sys

DEFAULT_BIND = ("127.0.2.1", 15300)


def servfail_response(query: bytes) -> bytes:
    if len(query) < 12:
        return b""
    qdcount = int.from_bytes(query[4:6], "big")
    if qdcount == 0:
        return b""
    # QR=1, RCODE=SERVFAIL (2)
    flags = (0x8000 | 0x0002).to_bytes(2, "big")
    header = query[0:2] + flags + query[4:6] + b"\x00\x00\x00\x00\x00\x00"
    offset = 12
    for _ in range(qdcount):
        if offset >= len(query):
            break
        while offset < len(query) and query[offset] != 0:
            offset += 1 + query[offset]
        offset += 1 + 4  # null label + QTYPE + QCLASS
    return header + query[12:offset]


def main() -> None:
    bind = DEFAULT_BIND
    if len(sys.argv) >= 2:
        bind = (DEFAULT_BIND[0], int(sys.argv[1]))
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(bind)
    print(f"mock-upstream-servfail listening on {bind[0]}:{bind[1]}", flush=True)
    while True:
        data, addr = sock.recvfrom(4096)
        resp = servfail_response(data)
        if resp:
            sock.sendto(resp, addr)


if __name__ == "__main__":
    main()
