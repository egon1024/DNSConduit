"""Takeaway integrity: generated relative deltas + numeric/banned-phrase checks.

Gate G5 / design D4 — prefer machine-checkable claims derived from study figure
charts (same reference JSON as Evidence tables). Hand-authored Takeaway prose may
restate those numbers within documented rounding tolerance.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any

from perf.render.charts import ChartSpec

# Documented secondary lint: obsolete host-noise / inversion framing after the
# elevated + median + governor / integrity promotes. Keep this list small.
BANNED_TAKEAWAY_PHRASES: tuple[str, ...] = (
    "same-host noise",
    "treat as noise",
    "treat that as noise",
    "treat those as noise",
    "noisy under default outstanding",
    "thin-default inversion",
    "thin default inversion",
    "feature-tax inversion",
    "inversion under thin",
    "ladder-only elevated",
)

# Rounding / match tolerances (design D4 + G1c spot-check practice).
QPS_K_TOLERANCE = 1  # ±1k when matching ~Nk
PERCENT_TOLERANCE = 1.0  # ±1 percentage point
MULTIPLIER_TOLERANCE = 0.05  # 1.9× matches 1.878
MS_TOLERANCE = 1.0  # ±1 ms
# When a ratio is near an integer, also allow "~2×" style claims.
NEAR_INTEGER_MULT = 0.15

_TAKEAWAY_RE = re.compile(
    r"^##\s+Takeaway\s*$([\s\S]*?)(?=^##\s+|\Z)", re.MULTILINE
)
_MULT_RE = re.compile(r"~?\*{0,2}(\d+(?:\.\d+)?)\*{0,2}\s*×")
_PCT_RE = re.compile(r"~?\*{0,2}(\d+(?:\.\d+)?)\*{0,2}\s*%")
_QPS_K_RE = re.compile(r"~?\*{0,2}(\d+)\*{0,2}\s*k\b", re.IGNORECASE)
_MS_RE = re.compile(r"~?\*{0,2}(\d+(?:\.\d+)?)\*{0,2}\s*ms\b", re.IGNORECASE)
# Non-evidence percents: sample rates / "10% of responses".
_PCT_SAMPLE_OR_OF_RE = re.compile(
    r"%\s+of\b|"
    r"\bsampl(?:e|ing|ed)\b|"
    r"\bsample_percent\b",
    re.IGNORECASE,
)
# Cross-study citation in the same sentence (another studies/*.md link).
_CROSS_STUDY_LINK_RE = re.compile(
    r"/performance/studies/[a-z0-9_-]+", re.IGNORECASE
)


@dataclass
class AllowedClaims:
    """Numeric claims a Takeaway may assert for one study."""

    qps_thousands: set[int] = field(default_factory=set)
    percents: set[float] = field(default_factory=set)
    multipliers: set[float] = field(default_factory=set)
    durations_ms: set[float] = field(default_factory=set)


def takeaway_section(page_text: str) -> str:
    match = _TAKEAWAY_RE.search(page_text)
    if not match:
        return ""
    return match.group(1)


_CACHE_HIT_METRICS = frozenset(
    {
        "cache_hit_rate",
        "cache_lookups_hit",
        "cache_lookups_miss",
    }
)
_CACHE_PATH_DURATION_METRICS = frozenset(
    {
        "cache_fill_duration_mean_ms",
        "cache_eviction_duration_mean_ms",
    }
)


def _series_values(chart: ChartSpec) -> list[float | None]:
    if not chart.series:
        return [None] * len(chart.categories)
    return list(chart.series[0][1])


def _chart_metric(chart: ChartSpec, fallback: str) -> str:
    if chart.series:
        return chart.series[0][0]
    return fallback


def _round_qps_k(qps: float) -> int:
    return int(round(qps / 1000.0))


def _fmt_qps_plain(qps: float) -> str:
    """Plain QPS label (~Nk, or absolute when under 500)."""
    if qps < 500:
        return f"~{int(round(qps))} QPS"
    return f"~{_round_qps_k(qps)}k"


def _fmt_hit_rate(rate: float) -> str:
    return f"~{rate:.1f}%"


def _fmt_ms_plain(ms: float) -> str:
    if ms < 0.01:
        return f"~{ms:.4f} ms"
    if ms < 1.0:
        return f"~{ms:.3f} ms"
    return f"~{ms:.1f} ms"


def _round_mult(ratio: float) -> float:
    return round(ratio, 1)


def _add_multiplier(claims: AllowedClaims, ratio: float) -> None:
    if ratio <= 0:
        return
    claims.multipliers.add(_round_mult(ratio))
    nearest = float(round(ratio))
    if nearest >= 1.0 and abs(ratio - nearest) <= NEAR_INTEGER_MULT:
        claims.multipliers.add(nearest)


def _round_pct(pct: float) -> float:
    return float(round(pct))


def claims_from_charts(
    charts: list[ChartSpec],
    *,
    primary_metric: str,
) -> AllowedClaims:
    """Derive allowed Takeaway numbers from figure series (Evidence poles)."""
    claims = AllowedClaims()

    for chart in charts:
        metric = _chart_metric(chart, primary_metric)
        values = _series_values(chart)
        ok = [v for v in values if v is not None]
        if metric == "drain_duration_ms" or metric in _CACHE_PATH_DURATION_METRICS:
            for v in ok:
                # Sub-ms path timings keep finer precision for integrity.
                if metric in _CACHE_PATH_DURATION_METRICS and v < 1.0:
                    claims.durations_ms.add(round(v, 4))
                else:
                    claims.durations_ms.add(float(round(v)))
            if metric in _CACHE_PATH_DURATION_METRICS and len(ok) >= 2:
                baseline = ok[0]
                if baseline > 0:
                    for v in ok[1:]:
                        if v > 0:
                            _add_multiplier(claims, v / baseline)
                            _add_multiplier(claims, baseline / v)
                            _add_multiplier(
                                claims, max(v, baseline) / min(v, baseline)
                            )
            continue

        if metric in _CACHE_HIT_METRICS:
            if len(ok) >= 2:
                baseline = ok[0]
                if baseline > 0:
                    for v in ok[1:]:
                        if v <= 0:
                            continue
                        _add_multiplier(claims, v / baseline)
                        _add_multiplier(claims, baseline / v)
                        claims.percents.add(
                            _round_pct(abs(baseline - v) / baseline * 100.0)
                        )
            continue

        for v in ok:
            claims.qps_thousands.add(_round_qps_k(v))

        if len(ok) < 2:
            continue

        baseline = ok[0]
        if baseline <= 0:
            continue
        for v in ok[1:]:
            if v <= 0:
                continue
            _add_multiplier(claims, v / baseline)
            _add_multiplier(claims, baseline / v)
            _add_multiplier(claims, max(v, baseline) / min(v, baseline))
            claims.percents.add(_round_pct(abs(baseline - v) / baseline * 100.0))

        # Consecutive step ratios (ingress series: ~2× each).
        for a, b in zip(ok, ok[1:]):
            if a and b and a > 0 and b > 0:
                _add_multiplier(claims, b / a)
                _add_multiplier(claims, a / b)
                claims.percents.add(_round_pct(abs(b - a) / a * 100.0))

    return claims


def _percent_claim_is_non_evidence(section: str, match: re.Match[str]) -> bool:
    """Skip sample-rate / cross-study percent mentions (not this page's poles)."""
    # "~10% of responses", "100% sampling"
    window_start = max(0, match.start() - 48)
    window_end = min(len(section), match.end() + 48)
    window = section[window_start:window_end]
    if _PCT_SAMPLE_OR_OF_RE.search(window):
        return True
    # Cross-study cite may wrap lines; search a wider neighborhood for a studies link.
    cite_start = max(0, match.start() - 160)
    cite_end = min(len(section), match.end() + 200)
    if _CROSS_STUDY_LINK_RE.search(section[cite_start:cite_end]):
        return True
    return False


def _md_pole(label: str) -> str:
    """Format a category label for operator prose (backticks when token-like)."""
    text = str(label).strip()
    if not text:
        return "`?`"
    if re.fullmatch(r"[A-Za-z0-9_./:-]+", text):
        return f"`{text}`"
    return text


_CHART_TITLE_PREFIXES = (
    "Achieved QPS — ",
    "Feature tax — ",
)


def _chart_heading(chart: ChartSpec) -> str:
    """Short operator label from the figure title (drop chart-catalog prefixes)."""
    title = (chart.title or "").strip()
    if not title:
        return chart.id.replace("-", " ")
    for prefix in _CHART_TITLE_PREFIXES:
        if title.startswith(prefix):
            return title[len(prefix) :]
    return title


def format_delta_fragment(
    *,
    study_id: str,
    charts: list[ChartSpec],
    claims: AllowedClaims,
    primary_metric: str,
) -> str:
    """Markdown body for ``study-{id}-deltas.fragment.md`` (injected above Takeaway)."""
    del claims  # reserved for future richer fragments; series drive bullets today
    # No lead sentence: the page already carries the same-host disclaimer,
    # and Takeaway/Evidence jump straight into content.
    lines = [
        "## At a glance",
        "",
    ]
    any_bullet = False

    for chart in charts:
        metric = _chart_metric(chart, primary_metric)
        values = _series_values(chart)
        cats = chart.categories
        ok_pairs = [
            (cats[i] if i < len(cats) else f"pole-{i}", values[i])
            for i in range(len(values))
            if values[i] is not None
        ]
        heading = _chart_heading(chart)
        if not ok_pairs:
            lines.append(
                f"- **{heading}:** no published comparison yet "
                "(those cells were not promoted)."
            )
            any_bullet = True
            continue

        if metric == "drain_duration_ms":
            bits = ", ".join(
                f"{_md_pole(label)} ≈ **{int(round(val))} ms**"
                for label, val in ok_pairs
            )
            lines.append(f"- **{heading}:** {bits}")
            any_bullet = True
            continue

        if metric in _CACHE_PATH_DURATION_METRICS:
            if len(ok_pairs) == 1:
                label, val = ok_pairs[0]
                lines.append(
                    f"- **{heading}:** only {_md_pole(label)} is published "
                    f"({_fmt_ms_plain(val)}); no paired comparison on this reference."
                )
            else:
                base_label, baseline = ok_pairs[0]
                parts: list[str] = []
                for label, val in ok_pairs[1:]:
                    if baseline <= 0 or val <= 0:
                        continue
                    ratio = val / baseline
                    if ratio >= 100.0 or ratio <= 0.01:
                        parts.append(
                            f"{_md_pole(label)} ≈ {_fmt_ms_plain(val)} vs "
                            f"{_md_pole(base_label)} {_fmt_ms_plain(baseline)}"
                        )
                    elif val >= baseline:
                        parts.append(
                            f"{_md_pole(label)} is about "
                            f"**{_round_mult(ratio)}×** "
                            f"{_md_pole(base_label)} "
                            f"({_fmt_ms_plain(val)} vs {_fmt_ms_plain(baseline)})"
                        )
                    else:
                        parts.append(
                            f"{_md_pole(label)} is about "
                            f"**{_round_pct(abs(baseline - val) / baseline * 100.0):.0f}%** "
                            f"faster than {_md_pole(base_label)} "
                            f"({_fmt_ms_plain(val)} vs {_fmt_ms_plain(baseline)})"
                        )
                if not parts:
                    lines.append(
                        f"- **{heading}:** {_md_pole(base_label)} "
                        f"≈ {_fmt_ms_plain(baseline)}."
                    )
                else:
                    lines.append(f"- **{heading}:** " + "; ".join(parts) + ".")
            any_bullet = True
            continue

        if metric in _CACHE_HIT_METRICS:
            if len(ok_pairs) == 1:
                label, val = ok_pairs[0]
                lines.append(
                    f"- **{heading}:** only {_md_pole(label)} is published "
                    f"({_fmt_hit_rate(val)}); no paired comparison on this reference."
                )
            else:
                base_label, baseline = ok_pairs[0]
                parts: list[str] = []
                for label, val in ok_pairs[1:]:
                    if baseline <= 0:
                        continue
                    delta_pct = abs(baseline - val) / baseline * 100.0
                    if val >= baseline:
                        parts.append(
                            f"{_md_pole(label)} is about "
                            f"**{_round_pct(delta_pct):.0f}%** higher hit rate "
                            f"than {_md_pole(base_label)} "
                            f"({_fmt_hit_rate(val)} vs {_fmt_hit_rate(baseline)})"
                        )
                    else:
                        parts.append(
                            f"{_md_pole(label)} is about "
                            f"**{_round_pct(delta_pct):.0f}%** lower hit rate "
                            f"than {_md_pole(base_label)} "
                            f"({_fmt_hit_rate(val)} vs {_fmt_hit_rate(baseline)})"
                        )
                if not parts:
                    lines.append(
                        f"- **{heading}:** {_md_pole(base_label)} "
                        f"≈ {_fmt_hit_rate(baseline)}."
                    )
                else:
                    lines.append(f"- **{heading}:** " + "; ".join(parts) + ".")
            any_bullet = True
            continue

        if len(ok_pairs) == 1:
            label, val = ok_pairs[0]
            lines.append(
                f"- **{heading}:** only {_md_pole(label)} is published "
                f"({_fmt_qps_plain(val)}); no paired comparison on this reference."
            )
            any_bullet = True
            continue

        base_label, baseline = ok_pairs[0]
        parts = []
        for label, val in ok_pairs[1:]:
            if baseline <= 0 or val <= 0:
                continue
            if val >= baseline:
                parts.append(
                    f"{_md_pole(label)} is about "
                    f"**{_round_mult(val / baseline)}×** "
                    f"{_md_pole(base_label)} "
                    f"({_fmt_qps_plain(val)} vs {_fmt_qps_plain(baseline)})"
                )
            else:
                parts.append(
                    f"{_md_pole(label)} costs about "
                    f"**{_round_pct(abs(baseline - val) / baseline * 100.0):.0f}%** "
                    f"QPS versus {_md_pole(base_label)} "
                    f"({_fmt_qps_plain(val)} vs {_fmt_qps_plain(baseline)})"
                )
        if not parts:
            lines.append(
                f"- **{heading}:** {_md_pole(base_label)} "
                f"≈ {_fmt_qps_plain(baseline)}."
            )
        else:
            lines.append(f"- **{heading}:** " + "; ".join(parts) + ".")
        any_bullet = True

    if not any_bullet:
        lines.append(
            f"- No same-host comparison is published for `{study_id}` yet."
        )
    lines.append("")
    return "\n".join(lines)


def _matches_float(claim: float, allowed: set[float], tol: float) -> bool:
    return any(abs(claim - a) <= tol for a in allowed)


def _matches_int(claim: int, allowed: set[int], tol: int) -> bool:
    return any(abs(claim - a) <= tol for a in allowed)


def check_takeaway_numeric_claims(
    page_text: str,
    claims: AllowedClaims,
    *,
    study_id: str,
) -> list[str]:
    """Return error strings for Takeaway numbers that disagree with Evidence."""
    section = takeaway_section(page_text)
    if not section.strip():
        return []

    errors: list[str] = []

    for match in _MULT_RE.finditer(section):
        value = float(match.group(1))
        if not claims.multipliers or not _matches_float(
            value, claims.multipliers, MULTIPLIER_TOLERANCE
        ):
            errors.append(
                f"{study_id}: takeaway multiplier {value}× is not supported by "
                f"evidence (allowed ≈ {sorted(claims.multipliers)})"
            )

    for match in _PCT_RE.finditer(section):
        value = float(match.group(1))
        if claims.percents and _matches_float(
            value, claims.percents, PERCENT_TOLERANCE
        ):
            continue
        # Unmatched %: allow sample-rate / cross-study citations; else fail.
        if _percent_claim_is_non_evidence(section, match):
            continue
        errors.append(
            f"{study_id}: takeaway percent {value}% is not supported by "
            f"evidence (allowed ≈ {sorted(claims.percents)})"
        )

    for match in _QPS_K_RE.finditer(section):
        value = int(match.group(1))
        if not claims.qps_thousands or not _matches_int(
            value, claims.qps_thousands, QPS_K_TOLERANCE
        ):
            errors.append(
                f"{study_id}: takeaway ~{value}k QPS is not supported by "
                f"evidence (allowed ≈ {sorted(claims.qps_thousands)}k)"
            )

    for match in _MS_RE.finditer(section):
        value = float(match.group(1))
        if not claims.durations_ms or not _matches_float(
            value, claims.durations_ms, MS_TOLERANCE
        ):
            errors.append(
                f"{study_id}: takeaway {value} ms is not supported by "
                f"evidence (allowed ≈ {sorted(claims.durations_ms)} ms)"
            )

    return errors


def check_banned_phrases(text: str, *, study_id: str) -> list[str]:
    """Secondary lint: obsolete noise/inversion claims in Takeaway prose."""
    if not text:
        return []
    lowered = text.lower()
    errors: list[str] = []
    for phrase in BANNED_TAKEAWAY_PHRASES:
        if phrase in lowered:
            errors.append(
                f"{study_id}: banned stale-claim phrase in takeaway: {phrase!r}"
            )
    return errors


def check_study_page(
    page_text: str,
    charts: list[ChartSpec],
    *,
    study_id: str,
    primary_metric: str,
) -> list[str]:
    claims = claims_from_charts(charts, primary_metric=primary_metric)
    section = takeaway_section(page_text)
    errors = check_takeaway_numeric_claims(page_text, claims, study_id=study_id)
    errors.extend(check_banned_phrases(section, study_id=study_id))
    return errors


class TakeawayIntegrityError(RuntimeError):
    """Raised when generate-docs finds takeaway/evidence conflicts."""

    def __init__(self, errors: list[str]):
        self.errors = list(errors)
        super().__init__(
            "takeaway integrity check failed:\n" + "\n".join(f"  - {e}" for e in errors)
        )


def verify_studies_integrity(
    studies_with_charts: list[tuple[Any, list[ChartSpec]]],
    *,
    page_text_by_id: dict[str, str],
) -> list[str]:
    """Check all published studies; return aggregated error messages."""
    errors: list[str] = []
    for study, charts in studies_with_charts:
        page = page_text_by_id.get(study.id)
        if page is None:
            errors.append(f"{study.id}: study page missing for integrity check")
            continue
        errors.extend(
            check_study_page(
                page,
                charts,
                study_id=study.id,
                primary_metric=study.primary_metric,
            )
        )
    return errors
