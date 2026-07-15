"""Count upstream queries from peer daemon logs (cache-hit proof)."""

from __future__ import annotations

import re

# dnsmasq --log-queries: "query[A] www.smoke.test from 172.30.97.20"
_DNSMASQ_QUERY = re.compile(
    r"query\[(?P<qtype>[A-Za-z0-9]+)\]\s+(?P<qname>\S+)\s+from\s+(?P<from>\S+)"
)


def normalize_qname(qname: str) -> str:
    return qname.strip().rstrip(".").lower()


def count_dnsmasq_queries(
    log_text: str,
    qname: str,
    qtype: str = "A",
    *,
    from_ip: str | None = None,
) -> int:
    """Return how many dnsmasq query log lines match qname/qtype (optional client IP)."""
    want_name = normalize_qname(qname)
    want_type = qtype.strip().upper()
    count = 0
    for line in log_text.splitlines():
        m = _DNSMASQ_QUERY.search(line)
        if not m:
            continue
        if m.group("qtype").upper() != want_type:
            continue
        if normalize_qname(m.group("qname")) != want_name:
            continue
        if from_ip is not None and m.group("from") != from_ip:
            continue
        count += 1
    return count
