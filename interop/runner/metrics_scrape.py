"""Prometheus scrape text helpers for interop metrics-delta oracles."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Iterable
from urllib.error import URLError
from urllib.request import urlopen

# metric{labels} value  or  metric value
_SAMPLE = re.compile(
    r"^(?P<name>[a-zA-Z_:][a-zA-Z0-9_:]*)"
    r"(?:\{(?P<labels>[^}]*)\})?\s+"
    r"(?P<value>[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)"
)
_LABEL = re.compile(r'([a-zA-Z_][a-zA-Z0-9_]*)="((?:\\.|[^"\\])*)"')


@dataclass(frozen=True)
class PromSample:
    name: str
    labels: dict[str, str]
    value: float


def parse_labels(raw: str | None) -> dict[str, str]:
    if not raw:
        return {}
    out: dict[str, str] = {}
    for m in _LABEL.finditer(raw):
        key = m.group(1)
        val = m.group(2).replace(r"\\", "\\").replace(r"\"", '"').replace(r"\n", "\n")
        out[key] = val
    return out


def parse_prom_text(text: str) -> list[PromSample]:
    samples: list[PromSample] = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        m = _SAMPLE.match(line)
        if not m:
            continue
        samples.append(
            PromSample(
                name=m.group("name"),
                labels=parse_labels(m.group("labels")),
                value=float(m.group("value")),
            )
        )
    return samples


def sum_matching(
    samples: Iterable[PromSample],
    name: str,
    labels: dict[str, str] | None = None,
) -> float:
    """Sum sample values whose name matches and whose labels are a superset of ``labels``."""
    want = labels or {}
    total = 0.0
    for s in samples:
        if s.name != name:
            continue
        if any(s.labels.get(k) != v for k, v in want.items()):
            continue
        total += s.value
    return total


@dataclass
class MetricSamples:
    samples: list[PromSample]

    @classmethod
    def from_text(cls, text: str) -> MetricSamples:
        return cls(samples=parse_prom_text(text))

    def sum(self, name: str, labels: dict[str, str] | None = None) -> float:
        return sum_matching(self.samples, name, labels)


def scrape_metrics(url: str, *, timeout: float = 2.0) -> MetricSamples:
    try:
        with urlopen(url, timeout=timeout) as resp:  # noqa: S310 — lab scrape of compose-published port
            body = resp.read().decode("utf-8", errors="replace")
    except (URLError, TimeoutError, OSError) as exc:
        raise RuntimeError(f"metrics scrape failed for {url}: {exc}") from exc
    return MetricSamples.from_text(body)
