"""Shared chart models and emitters for operator-docs, rich, and HTML."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


SERIES_COLORS = ("#3949ab", "#00897b", "#f9a825", "#6d4c41", "#8e24aa", "#039be5")


@dataclass
class ChartSpec:
    """One comparison chart with optional rich table rows."""

    id: str
    title: str
    y_label: str
    categories: list[str]
    series: list[tuple[str, list[float | None]]]
    table_headers: list[str] = field(default_factory=list)
    table_rows: list[list[Any]] = field(default_factory=list)
    csv_headers: list[str] = field(default_factory=list)
    csv_rows: list[list[Any]] = field(default_factory=list)
    unavailable_note: str | None = None

    @property
    def has_data(self) -> bool:
        if self.unavailable_note:
            return False
        return any(v is not None for _, vals in self.series for v in vals)


def scenario_map(doc: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {sc["id"]: sc for sc in doc.get("scenarios") or []}


def qps(sc: dict[str, Any] | None) -> float | None:
    if not sc or sc.get("status") != "ok":
        return None
    value = (sc.get("metrics") or {}).get("achieved_qps")
    return float(value) if isinstance(value, (int, float)) else None


def lat_avg(sc: dict[str, Any] | None) -> float | None:
    if not sc or sc.get("status") != "ok":
        return None
    value = ((sc.get("metrics") or {}).get("latency_ms") or {}).get("avg")
    return float(value) if isinstance(value, (int, float)) else None


def metric(sc: dict[str, Any] | None, key: str) -> float | None:
    if not sc or sc.get("status") != "ok":
        return None
    value = (sc.get("metrics") or {}).get(key)
    return float(value) if isinstance(value, (int, float)) else None


def secondary(sc: dict[str, Any] | None, key: str) -> float | None:
    if not sc or sc.get("status") != "ok":
        return None
    value = (sc.get("secondary") or {}).get(key)
    return float(value) if isinstance(value, (int, float)) else None


def fmt(value: float | int | None, digits: int = 1) -> str:
    if value is None:
        return ""
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    return f"{float(value):.{digits}f}"


def scenario_md_link(scenario_id: str, label: str | None = None) -> str:
    text = label if label is not None else scenario_id
    return f"[{text}](/performance/scenarios.md#{scenario_id})"


def xml_escape(text: str) -> str:
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def svg_grouped_bars(
    *,
    title: str,
    categories: list[str],
    series: list[tuple[str, list[float | None]]],
    y_label: str,
    width: int = 640,
    height: int = 320,
) -> str:
    """Minimal static SVG grouped bar chart (no JS). Missing values omitted."""
    margin_l, margin_r, margin_t, margin_b = 56, 24, 40, 56
    plot_w = width - margin_l - margin_r
    plot_h = height - margin_t - margin_b
    values = [v for _, vals in series for v in vals if v is not None]
    ymax = max(values) if values else 1.0
    if ymax <= 0:
        ymax = 1.0
    n_cat = max(len(categories), 1)
    n_ser = max(len(series), 1)
    group_w = plot_w / n_cat
    bar_w = group_w / (n_ser + 1)

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" role="img" aria-label="{xml_escape(title)}">',
        '<rect width="100%" height="100%" fill="#fafafa"/>',
        f'<text x="{width // 2}" y="24" text-anchor="middle" '
        f'font-family="sans-serif" font-size="14" fill="#212121">{xml_escape(title)}</text>',
        f'<text x="12" y="{margin_t + plot_h // 2}" text-anchor="middle" '
        f'font-family="sans-serif" font-size="11" fill="#616161" '
        f'transform="rotate(-90 12 {margin_t + plot_h // 2})">{xml_escape(y_label)}</text>',
        f'<line x1="{margin_l}" y1="{margin_t}" x2="{margin_l}" '
        f'y2="{margin_t + plot_h}" stroke="#9e9e9e"/>',
        f'<line x1="{margin_l}" y1="{margin_t + plot_h}" x2="{margin_l + plot_w}" '
        f'y2="{margin_t + plot_h}" stroke="#9e9e9e"/>',
    ]
    for frac in (0.0, 0.5, 1.0):
        y = margin_t + plot_h * (1 - frac)
        val = ymax * frac
        parts.append(
            f'<line x1="{margin_l}" y1="{y:.1f}" x2="{margin_l + plot_w}" '
            f'y2="{y:.1f}" stroke="#eeeeee"/>'
        )
        parts.append(
            f'<text x="{margin_l - 6}" y="{y + 4:.1f}" text-anchor="end" '
            f'font-family="sans-serif" font-size="10" fill="#757575">'
            f"{val:.0f}</text>"
        )

    for ci, cat in enumerate(categories):
        gx = margin_l + group_w * ci
        for si, (label, vals) in enumerate(series):
            if ci >= len(vals) or vals[ci] is None:
                continue
            v = float(vals[ci])
            bh = (v / ymax) * plot_h
            x = gx + bar_w * (si + 0.5)
            y = margin_t + plot_h - bh
            color = SERIES_COLORS[si % len(SERIES_COLORS)]
            parts.append(
                f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w * 0.85:.1f}" '
                f'height="{bh:.1f}" fill="{color}">'
                f"<title>{xml_escape(label)} / {xml_escape(cat)}: {v:.2f}</title>"
                f"</rect>"
            )
        parts.append(
            f'<text x="{gx + group_w / 2:.1f}" y="{margin_t + plot_h + 18}" '
            f'text-anchor="middle" font-family="sans-serif" font-size="11" '
            f'fill="#424242">{xml_escape(cat)}</text>'
        )

    lx = margin_l
    for si, (label, _) in enumerate(series):
        color = SERIES_COLORS[si % len(SERIES_COLORS)]
        parts.append(
            f'<rect x="{lx}" y="{height - 22}" width="12" height="12" fill="{color}"/>'
        )
        parts.append(
            f'<text x="{lx + 16}" y="{height - 12}" font-family="sans-serif" '
            f'font-size="11" fill="#424242">{xml_escape(label)}</text>'
        )
        lx += 12 + 8 + len(label) * 7 + 16

    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def _fmt_bar_value(v: float) -> str:
    if v >= 100:
        return f"{v:.0f}"
    return f"{v:.1f}"


# Short display names for common long category ids (full id still in scenario table).
_BAR_LABEL_ALIASES: dict[str, str] = {
    "drain_complete": "complete",
    "drain_budgeted": "budgeted",
    "drain_minimal": "minimal",
    "metrics_off": "off",
    "minimal_scrape": "minimal",
    "standard_scrape": "standard",
    "dnstap_off": "off",
    "dnstap_sampled": "sampled",
    "dnstap_full": "full",
    "no_collect": "no_collect",
    "collect_only": "collect_only",
    "collect_emit": "collect_emit",
    "standard_dnstap_full": "both",
    "scrape_hammer": "hammer",
    "logging_warn": "warn",
    "logging_debug": "debug",
    "tracing_on": "tracing",

    "baseline_2w": "2w base",
    "topology_4w": "4w topo",
    "forward_fast": "fast",
    "forward_slow": "slow",
    "cache_hit": "cache_hit",
    "cold_start": "cold_start",
    "config_apply": "apply",
}


def _display_bar_label(name: str) -> str:
    return _BAR_LABEL_ALIASES.get(name, name)


def unicode_bars(
    *,
    title: str,
    categories: list[str],
    series: list[tuple[str, list[float | None]]],
    height: int = 10,
    col_width: int | None = None,
) -> list[str]:
    """Vertical Unicode bar charts for terminal rich output (SVG-like orientation)."""
    columns: list[tuple[str, str, float]] = []
    for ci, cat in enumerate(categories):
        for label, vals in series:
            if ci >= len(vals) or vals[ci] is None:
                continue
            columns.append((cat, label, float(vals[ci])))
    if not columns:
        return [title] if title else []

    ymax = max(v for _, _, v in columns)
    if ymax <= 0:
        ymax = 1.0

    multi_series = len({label for label, _ in series}) > 1
    display_names = [
        _display_bar_label(label if multi_series else cat) for cat, label, _ in columns
    ]
    value_strs = [_fmt_bar_value(v) for _, _, v in columns]
    # Size columns to fit labels and values (no silent truncation).
    if col_width is None:
        body = max(
            6,
            max(len(n) for n in display_names),
            max(len(s) for s in value_strs),
        )
        body = min(body + 1, 16)
    else:
        body = max(col_width, 4)

    gap = 2
    scaled = [(v / ymax) * height for _, _, v in columns]

    plot_rows: list[str] = []
    for level in range(height, 0, -1):
        parts: list[str] = []
        prev_cat: str | None = None
        for i, (cat, _label, _v) in enumerate(columns):
            if prev_cat is not None and cat != prev_cat:
                parts.append(" " * gap)
            prev_cat = cat
            h = scaled[i]
            if h >= level:
                ch = "█"
            elif h >= level - 0.5:
                ch = "▄"
            else:
                ch = " "
            parts.append(f" {ch * 2} ".center(body)[:body])
        if level == height:
            axis = f"{ymax:>7.0f} ┤"
        elif level == max(1, height // 2):
            axis = f"{ymax / 2:>7.0f} ┤"
        elif level == 1:
            axis = f"{0:>7d} ┤"
        else:
            axis = "        │"
        plot_rows.append(axis + "".join(parts))

    base_parts: list[str] = []
    prev_cat = None
    for cat, _label, _v in columns:
        if prev_cat is not None and cat != prev_cat:
            base_parts.append("─" * gap)
        prev_cat = cat
        base_parts.append("─" * body)
    baseline = "        └" + "".join(base_parts)

    label_parts: list[str] = []
    value_parts: list[str] = []
    prev_cat = None
    for i, (cat, _label, _v) in enumerate(columns):
        if prev_cat is not None and cat != prev_cat:
            label_parts.append(" " * gap)
            value_parts.append(" " * gap)
        prev_cat = cat
        label_parts.append(display_names[i].center(body)[:body])
        value_parts.append(value_strs[i].center(body)[:body])

    cat_header = ""
    if multi_series:
        chunks: list[str] = []
        i = 0
        while i < len(columns):
            cat = columns[i][0]
            if chunks:
                chunks.append(" " * gap)
            j = i
            while j < len(columns) and columns[j][0] == cat:
                j += 1
            width = (j - i) * body
            chunks.append(_display_bar_label(cat).center(width)[:width])
            i = j
        cat_header = "         " + "".join(chunks)

    lines: list[str] = []
    if title:
        lines.append(title)
    lines.extend(plot_rows)
    lines.append(baseline)
    if cat_header:
        lines.append(cat_header)
    lines.append("         " + "".join(label_parts))
    lines.append("         " + "".join(value_parts))
    if multi_series:
        lines.append(
            "  series: "
            + ", ".join(_display_bar_label(label) for label, _ in series)
        )
    return lines


_ANSI_RE = __import__("re").compile(r"\033\[[0-9;]*m")


def visible_width(text: str) -> int:
    return len(_ANSI_RE.sub("", text))


def pad_visible(text: str, width: int) -> str:
    return text + (" " * max(0, width - visible_width(text)))


def side_by_side_panels(
    left: list[str],
    right: list[str],
    *,
    gap: int = 4,
) -> list[str]:
    """Place two chart panels horizontally (for wide terminals)."""
    if not left:
        return list(right)
    if not right:
        return list(left)
    lw = max(visible_width(line) for line in left)
    height = max(len(left), len(right))
    out: list[str] = []
    for i in range(height):
        l = left[i] if i < len(left) else ""
        r = right[i] if i < len(right) else ""
        out.append(pad_visible(l, lw) + (" " * gap) + r)
    return out


def unicode_horizontal_bars(
    *,
    title: str,
    categories: list[str],
    series: list[tuple[str, list[float | None]]],
    width: int = 28,
) -> list[str]:
    """Horizontal Unicode bars (optional alternate)."""
    values = [v for _, vals in series for v in vals if v is not None]
    ymax = max(values) if values else 1.0
    if ymax <= 0:
        ymax = 1.0
    lines = [title]
    for ci, cat in enumerate(categories):
        for _si, (label, vals) in enumerate(series):
            if ci >= len(vals) or vals[ci] is None:
                continue
            v = float(vals[ci])
            filled = int(round((v / ymax) * width))
            bar = "█" * filled + "░" * (width - filled)
            lines.append(f"  {cat}/{label:<12} {bar} {v:.1f}")
    return lines


def md_table(headers: list[str], rows: list[list[Any]]) -> str:
    lines = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join("---" for _ in headers) + " |",
    ]
    for row in rows:
        cells = ["" if c is None else str(c) for c in row]
        lines.append("| " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def _rich_scale_row(
    sc: dict[str, Any] | None,
    *,
    link: bool,
    label: str | None = None,
) -> list[Any]:
    if not sc:
        return ["—", "—", "", "", "", "", "", ""]
    sid = sc.get("id", "")
    axes = sc.get("axes") or {}
    metrics = sc.get("metrics") or {}
    display = label if label is not None else sid
    name = scenario_md_link(sid, display) if link and sid else display
    runtime = axes.get("runtime", "")
    workers = []
    for key in ("ingress_workers", "policy_workers", "io_workers"):
        if key in axes:
            workers.append(f"{key.split('_')[0]}={axes[key]}")
    worker_s = ", ".join(workers)
    return [
        name,
        runtime,
        fmt(qps(sc)),
        fmt(lat_avg(sc)),
        fmt(metrics.get("queries_sent"), 0) if isinstance(metrics.get("queries_sent"), (int, float)) else "",
        fmt(metrics.get("queries_completed"), 0)
        if isinstance(metrics.get("queries_completed"), (int, float))
        else "",
        fmt(metrics.get("queries_lost"), 0)
        if isinstance(metrics.get("queries_lost"), (int, float))
        else "",
        worker_s,
    ]


_AXIS_TABLE_HEADERS = {
    "runtime": "Runtime",
    "obs_posture": "Posture",
    "ingress_workers": "Ingress workers",
    "io_workers": "I/O workers",
    "drain_policy": "Drain policy",
    "load_shape": "Load shape",
    "topology": "Topology",
}


def _axis_table_header(axis: str) -> str:
    if not axis or axis == "id":
        return "Scenario"
    return _AXIS_TABLE_HEADERS.get(axis, axis.replace("_", " ").title())


SCALE_TABLE_HEADERS = [
    "Scenario",
    "Runtime",
    "Achieved QPS",
    "Avg latency (ms)",
    "Sent",
    "Completed",
    "Lost",
    "Workers",
]
SCALE_CSV_HEADERS = [
    "scenario_id",
    "runtime",
    "achieved_qps",
    "latency_avg_ms",
    "queries_sent",
    "queries_completed",
    "queries_lost",
    "workers",
]


def build_scale_charts(
    smap: dict[str, dict[str, Any]],
    *,
    link_scenarios: bool = True,
) -> list[ChartSpec]:
    """One sync-vs-split_io chart per load shape so Y scales stay readable."""
    shapes = (
        (
            "forward_fast",
            "scale-sync-vs-split-io-forward-fast",
            "Achieved QPS — sync vs split_io (forward_fast)",
            "scale-sync-forward-fast",
            "scale-split-io-forward-fast",
        ),
        (
            "forward_slow",
            "scale-sync-vs-split-io-forward-slow",
            "Achieved QPS — sync vs split_io (forward_slow)",
            "scale-sync-forward-slow",
            "scale-split-io-forward-slow",
        ),
    )
    charts: list[ChartSpec] = []
    for shape, chart_id, title, sync_id, split_id in shapes:
        sync_sc = smap.get(sync_id)
        split_sc = smap.get(split_id)
        sync_q = qps(sync_sc)
        split_q = qps(split_sc)
        if sync_q is None and split_q is None:
            charts.append(
                ChartSpec(
                    id=chart_id,
                    title=title,
                    y_label="QPS",
                    categories=["sync", "split_io"],
                    series=[],
                    unavailable_note=(
                        f"_Scale sync vs split_io ({shape}) unavailable — "
                        "promoted reference lacks those results._"
                    ),
                )
            )
            continue
        md_rows = [
            _rich_scale_row(sync_sc, link=link_scenarios),
            _rich_scale_row(split_sc, link=link_scenarios),
        ]
        csv_rows = []
        for sc in (sync_sc, split_sc):
            if not sc:
                continue
            axes = sc.get("axes") or {}
            metrics = sc.get("metrics") or {}
            workers = []
            for key in ("ingress_workers", "policy_workers", "io_workers"):
                if key in axes:
                    workers.append(f"{axes[key]}")
            csv_rows.append(
                [
                    sc.get("id"),
                    axes.get("runtime", ""),
                    fmt(qps(sc)),
                    fmt(lat_avg(sc)),
                    metrics.get("queries_sent", ""),
                    metrics.get("queries_completed", ""),
                    metrics.get("queries_lost", ""),
                    "/".join(workers),
                ]
            )
        charts.append(
            ChartSpec(
                id=chart_id,
                title=title,
                y_label="QPS",
                categories=["sync", "split_io"],
                series=[
                    ("QPS", [sync_q, split_q]),
                ],
                table_headers=SCALE_TABLE_HEADERS,
                table_rows=md_rows,
                csv_headers=SCALE_CSV_HEADERS,
                csv_rows=csv_rows,
            )
        )
    return charts


def build_scale_extra_charts(
    smap: dict[str, dict[str, Any]],
    *,
    link_scenarios: bool = True,
) -> list[ChartSpec]:
    """Cache-hit and topology-heavy directional charts when present."""
    charts: list[ChartSpec] = []
    cache = smap.get("scale-sync-cache-hit")
    if qps(cache) is not None:
        charts.append(
            ChartSpec(
                id="scale-cache-hit",
                title="Achieved QPS — sync cache_hit",
                y_label="QPS",
                categories=["cache_hit"],
                series=[("sync", [qps(cache)])],
                table_headers=SCALE_TABLE_HEADERS,
                table_rows=[_rich_scale_row(cache, link=link_scenarios)],
                csv_headers=SCALE_CSV_HEADERS,
                csv_rows=[
                    [
                        cache.get("id") if cache else "",
                        (cache.get("axes") or {}).get("runtime", "") if cache else "",
                        fmt(qps(cache)),
                        fmt(lat_avg(cache)),
                        (cache.get("metrics") or {}).get("queries_sent", "") if cache else "",
                        (cache.get("metrics") or {}).get("queries_completed", "")
                        if cache
                        else "",
                        (cache.get("metrics") or {}).get("queries_lost", "") if cache else "",
                        "",
                    ]
                ],
            )
        )

    topo = smap.get("scale-split-io-topology-heavy")
    baseline = smap.get("scale-split-io-forward-fast")
    if qps(topo) is not None or qps(baseline) is not None:
        charts.append(
            ChartSpec(
                id="scale-topology-heavy",
                title="Achieved QPS — split_io topology (forward_fast)",
                y_label="QPS",
                categories=["baseline_2w", "topology_4w"],
                series=[("split_io", [qps(baseline), qps(topo)])],
                table_headers=SCALE_TABLE_HEADERS,
                table_rows=[
                    _rich_scale_row(baseline, link=link_scenarios),
                    _rich_scale_row(topo, link=link_scenarios),
                ],
                csv_headers=SCALE_CSV_HEADERS,
                csv_rows=[],
            )
        )
        # Fill csv from rows without markdown links
        csv_rows = []
        for sc in (baseline, topo):
            if not sc:
                continue
            axes = sc.get("axes") or {}
            metrics = sc.get("metrics") or {}
            workers = []
            for key in ("ingress_workers", "policy_workers", "io_workers"):
                if key in axes:
                    workers.append(str(axes[key]))
            csv_rows.append(
                [
                    sc.get("id"),
                    axes.get("runtime", ""),
                    fmt(qps(sc)),
                    fmt(lat_avg(sc)),
                    metrics.get("queries_sent", ""),
                    metrics.get("queries_completed", ""),
                    metrics.get("queries_lost", ""),
                    "/".join(workers),
                ]
            )
        charts[-1].csv_rows = csv_rows
    return charts


def build_drain_chart(
    smap: dict[str, dict[str, Any]],
    *,
    link_scenarios: bool = True,
) -> ChartSpec:
    order = [
        ("drain_complete", "shutdown-drain-complete-forward-slow"),
        ("drain_budgeted", "shutdown-drain-budgeted-forward-slow"),
        ("drain_minimal", "shutdown-drain-minimal-forward-slow"),
    ]
    categories = [p for p, _ in order]
    durations: list[float | None] = []
    md_rows: list[list[Any]] = []
    csv_rows: list[list[Any]] = []
    for policy, sid in order:
        sc = smap.get(sid)
        dur = metric(sc, "drain_duration_ms")
        loss = secondary(sc, "client_failures_during_stop")
        if loss is None:
            loss = metric(sc, "queries_lost")
        durations.append(dur)
        name = scenario_md_link(sid, policy) if link_scenarios and sc else policy
        md_rows.append(
            [
                name,
                fmt(dur),
                fmt(loss, 0),
                fmt(qps(sc)),
                fmt(lat_avg(sc)),
                fmt(metric(sc, "queries_sent"), 0),
                fmt(metric(sc, "queries_completed"), 0),
            ]
        )
        csv_rows.append(
            [
                policy,
                sid,
                fmt(dur),
                fmt(loss, 0),
                fmt(qps(sc)),
                fmt(lat_avg(sc)),
                fmt(metric(sc, "queries_sent"), 0),
                fmt(metric(sc, "queries_completed"), 0),
            ]
        )
    if all(v is None for v in durations):
        return ChartSpec(
            id="shutdown-drain-forward-slow",
            title="Drain duration under forward_slow",
            y_label="ms",
            categories=categories,
            series=[],
            unavailable_note=(
                "_Shutdown drain chart unavailable — "
                "promoted reference lacks drain policy results under forward_slow._"
            ),
        )
    return ChartSpec(
        id="shutdown-drain-forward-slow",
        title="Drain duration under forward_slow",
        y_label="ms",
        categories=categories,
        series=[("drain_duration_ms", durations)],
        table_headers=[
            "Drain policy",
            "Drain duration (ms)",
            "Client failures during stop",
            "QPS",
            "Avg latency (ms)",
            "Sent",
            "Completed",
        ],
        table_rows=md_rows,
        csv_headers=[
            "drain_policy",
            "scenario_id",
            "drain_duration_ms",
            "client_failures_during_stop",
            "achieved_qps",
            "latency_avg_ms",
            "queries_sent",
            "queries_completed",
        ],
        csv_rows=csv_rows,
    )


FEATURE_TAX_SCRAPE = (
    ("metrics_off", "feature-tax-metrics-off-scrape-ladder-forward-fast"),
    ("minimal_scrape", "feature-tax-metrics-minimal-scrape-ladder-forward-fast"),
    ("standard_scrape", "feature-tax-metrics-standard-scrape-ladder-forward-fast"),
)
FEATURE_TAX_DNSTAP = (
    ("dnstap_off", "feature-tax-dnstap-off-forward-fast"),
    ("dnstap_sampled", "feature-tax-dnstap-sampled-forward-fast"),
    ("dnstap_full", "feature-tax-dnstap-full-forward-fast"),
)
FEATURE_TAX_COLLECT_EMIT = (
    ("no_collect", "feature-tax-metrics-no-collect-forward-fast"),
    ("collect_only", "feature-tax-metrics-collect-only-forward-fast"),
    ("collect_emit", "feature-tax-metrics-collect-emit-forward-fast"),
)
FEATURE_TAX_COMBINED = (
    ("metrics_off", "feature-tax-metrics-off-forward-fast"),
    ("standard_scrape", "feature-tax-metrics-standard-scrape-forward-fast"),
    ("dnstap_full", "feature-tax-dnstap-full-forward-fast"),
    ("standard_dnstap_full", "feature-tax-metrics-standard-dnstap-full-forward-fast"),
)
FEATURE_TAX_SCRAPE_HAMMER = (
    ("metrics_off", "feature-tax-metrics-off-forward-fast"),
    ("standard_scrape", "feature-tax-metrics-standard-scrape-forward-fast"),
    ("scrape_hammer", "feature-tax-metrics-standard-scrape-hammer-forward-fast"),
)
FEATURE_TAX_SPLIT_IO_SCRAPE = (
    ("metrics_off", "feature-tax-metrics-off-split-io-forward-fast"),
    ("standard_scrape", "feature-tax-metrics-standard-scrape-split-io-forward-fast"),
)


def _feature_tax_chart(
    *,
    chart_id: str,
    title: str,
    pairs: tuple[tuple[str, str], ...],
    smap: dict[str, dict[str, Any]],
    link_scenarios: bool,
) -> ChartSpec | None:
    categories = [label for label, _ in pairs]
    values = [qps(smap.get(sid)) for _, sid in pairs]
    if all(v is None for v in values):
        return None
    md_rows: list[list[Any]] = []
    csv_rows: list[list[Any]] = []
    for label, sid in pairs:
        sc = smap.get(sid)
        name = scenario_md_link(sid, label) if link_scenarios and sc else label
        md_rows.append(
            [
                name,
                fmt(qps(sc)),
                fmt(lat_avg(sc)),
                fmt(metric(sc, "queries_sent"), 0),
                fmt(metric(sc, "queries_completed"), 0),
                fmt(metric(sc, "queries_lost"), 0),
            ]
        )
        csv_rows.append(
            [
                label,
                sid,
                fmt(qps(sc)),
                fmt(lat_avg(sc)),
                fmt(metric(sc, "queries_sent"), 0),
                fmt(metric(sc, "queries_completed"), 0),
                fmt(metric(sc, "queries_lost"), 0),
            ]
        )
    return ChartSpec(
        id=chart_id,
        title=title,
        y_label="QPS",
        categories=categories,
        series=[("QPS", values)],
        table_headers=[
            "Posture",
            "Achieved QPS",
            "Avg latency (ms)",
            "Sent",
            "Completed",
            "Lost",
        ],
        table_rows=md_rows,
        csv_headers=[
            "posture",
            "scenario_id",
            "achieved_qps",
            "latency_avg_ms",
            "queries_sent",
            "queries_completed",
            "queries_lost",
        ],
        csv_rows=csv_rows,
    )


def build_feature_tax_charts(
    smap: dict[str, dict[str, Any]],
    *,
    link_scenarios: bool = True,
) -> list[ChartSpec]:
    charts: list[ChartSpec] = []
    recipes: list[tuple[str, str, tuple[tuple[str, str], ...]]] = [
        (
            "feature-tax-metrics-scrape",
            "Feature tax — metrics scrape ladder (forward_fast)",
            FEATURE_TAX_SCRAPE,
        ),
        (
            "feature-tax-dnstap",
            "Feature tax — dnstap off / sampled / full (forward_fast)",
            FEATURE_TAX_DNSTAP,
        ),
        (
            "feature-tax-collect-emit",
            "Feature tax — collect vs emit (forward_fast)",
            FEATURE_TAX_COLLECT_EMIT,
        ),
        (
            "feature-tax-combined",
            "Feature tax — metrics and dnstap combined (forward_fast)",
            FEATURE_TAX_COMBINED,
        ),
        (
            "feature-tax-scrape-hammer",
            "Feature tax — scrape hammer under load (forward_fast)",
            FEATURE_TAX_SCRAPE_HAMMER,
        ),
        (
            "feature-tax-scrape-split-io",
            "Feature tax — metrics scrape under split_io (forward_fast)",
            FEATURE_TAX_SPLIT_IO_SCRAPE,
        ),
    ]
    for chart_id, title, pairs in recipes:
        chart = _feature_tax_chart(
            chart_id=chart_id,
            title=title,
            pairs=pairs,
            smap=smap,
            link_scenarios=link_scenarios,
        )
        if chart:
            charts.append(chart)
    return charts


def build_lifecycle_charts(
    smap: dict[str, dict[str, Any]],
    *,
    link_scenarios: bool = True,
) -> list[ChartSpec]:
    """Separate charts so cold-start ms does not dwarf config-apply."""
    charts: list[ChartSpec] = []
    cold = smap.get("lifecycle-cold-start")
    apply = smap.get("lifecycle-config-apply")
    cold_ms = metric(cold, "cold_start_ms")
    apply_ms = metric(apply, "apply_latency_ms")
    if cold_ms is not None and cold:
        name = (
            scenario_md_link("lifecycle-cold-start", "cold_start")
            if link_scenarios
            else "cold_start"
        )
        charts.append(
            ChartSpec(
                id="lifecycle-cold-start",
                title="Lifecycle — cold start",
                y_label="ms",
                categories=["cold_start"],
                series=[("ms", [cold_ms])],
                table_headers=["Metric", "Duration (ms)"],
                table_rows=[[name, fmt(cold_ms)]],
                csv_headers=["metric", "scenario_id", "duration_ms"],
                csv_rows=[["cold_start", "lifecycle-cold-start", fmt(cold_ms)]],
            )
        )
    if apply_ms is not None and apply:
        name = (
            scenario_md_link("lifecycle-config-apply", "config_apply")
            if link_scenarios
            else "config_apply"
        )
        charts.append(
            ChartSpec(
                id="lifecycle-config-apply",
                title="Lifecycle — config apply",
                y_label="ms",
                categories=["config_apply"],
                series=[("ms", [apply_ms])],
                table_headers=["Metric", "Duration (ms)"],
                table_rows=[[name, fmt(apply_ms)]],
                csv_headers=["metric", "scenario_id", "duration_ms"],
                csv_rows=[["config_apply", "lifecycle-config-apply", fmt(apply_ms)]],
            )
        )
    return charts


def charts_for_document(
    doc: dict[str, Any],
    *,
    link_scenarios: bool = True,
) -> list[ChartSpec]:
    """All curated charts present in a run/reference document."""
    smap = scenario_map(doc)
    out: list[ChartSpec] = []
    out.extend(build_scale_charts(smap, link_scenarios=link_scenarios))
    out.extend(build_scale_extra_charts(smap, link_scenarios=link_scenarios))
    drain = build_drain_chart(smap, link_scenarios=link_scenarios)
    out.append(drain)
    out.extend(build_feature_tax_charts(smap, link_scenarios=link_scenarios))
    out.extend(build_lifecycle_charts(smap, link_scenarios=link_scenarios))
    return out


# Curated promote keep-set (legacy thin spine / warehouse charts).
CURATED_SCENARIO_IDS: tuple[str, ...] = (
    "scale-sync-forward-fast",
    "scale-sync-forward-slow",
    "scale-split-io-forward-fast",
    "scale-split-io-forward-slow",
    "scale-sync-cache-hit",
    "scale-split-io-topology-heavy",
    "shutdown-drain-complete-forward-slow",
    "shutdown-drain-budgeted-forward-slow",
    "shutdown-drain-minimal-forward-slow",
    "feature-tax-metrics-off-forward-fast",
    "feature-tax-metrics-minimal-scrape-forward-fast",
    "feature-tax-metrics-standard-scrape-forward-fast",
    "feature-tax-metrics-off-scrape-ladder-forward-fast",
    "feature-tax-metrics-minimal-scrape-ladder-forward-fast",
    "feature-tax-metrics-standard-scrape-ladder-forward-fast",
    "feature-tax-dnstap-off-forward-fast",
    "feature-tax-dnstap-sampled-forward-fast",
    "feature-tax-dnstap-full-forward-fast",
    "feature-tax-metrics-no-collect-forward-fast",
    "feature-tax-metrics-collect-only-forward-fast",
    "feature-tax-metrics-collect-emit-forward-fast",
    "feature-tax-metrics-standard-dnstap-full-forward-fast",
    "feature-tax-metrics-otlp-push-forward-fast",
    "feature-tax-logging-warn-forward-fast",
    "feature-tax-logging-debug-forward-fast",
    "feature-tax-tracing-on-forward-fast",

    "feature-tax-metrics-off-split-io-forward-fast",
    "feature-tax-metrics-standard-scrape-split-io-forward-fast",
    "feature-tax-metrics-standard-scrape-hammer-forward-fast",
    "lifecycle-cold-start",
    "lifecycle-config-apply",
)


def _study_metric(sc: dict[str, Any] | None, key: str) -> float | None:
    if key in ("achieved_qps", "qps"):
        return qps(sc)
    if key in ("latency_avg_ms", "latency_ms"):
        return lat_avg(sc)
    if key == "drain_duration_ms":
        return metric(sc, "drain_duration_ms")
    return metric(sc, key)


def _category_label(sc: dict[str, Any] | None, axis: str, fallback: str) -> str:
    if not sc:
        return fallback
    axes = sc.get("axes") or {}
    if axis and axis in axes:
        return str(axes[axis])
    return fallback


def build_study_figure_chart(
    *,
    study_id: str,
    figure_id: str,
    title: str,
    y_label: str,
    member_ids: tuple[str, ...] | list[str],
    smap: dict[str, dict[str, Any]],
    primary_metric: str,
    compare_axis: str,
    category_axis: str | None = None,
    link_scenarios: bool = True,
) -> ChartSpec:
    """Build one ChartSpec from a study figure recipe + scenario map."""
    axis = category_axis or compare_axis or "id"
    categories: list[str] = []
    values: list[float | None] = []
    md_rows: list[list[Any]] = []
    csv_rows: list[list[Any]] = []
    for mid in member_ids:
        sc = smap.get(mid)
        categories.append(_category_label(sc, axis, mid))
        values.append(_study_metric(sc, primary_metric))
        cat = categories[-1]
        if primary_metric == "drain_duration_ms":
            loss = secondary(sc, "client_failures_during_stop")
            if loss is None:
                loss = metric(sc, "queries_lost")
            name = scenario_md_link(mid, cat) if link_scenarios and sc else cat
            md_rows.append(
                [
                    name,
                    fmt(_study_metric(sc, primary_metric)),
                    fmt(loss, 0),
                    fmt(qps(sc)),
                    fmt(lat_avg(sc)),
                ]
            )
            csv_rows.append(
                [
                    mid,
                    cat,
                    fmt(_study_metric(sc, primary_metric)),
                    fmt(loss, 0),
                    fmt(qps(sc)),
                    fmt(lat_avg(sc)),
                ]
            )
        else:
            row = _rich_scale_row(sc, link=link_scenarios, label=cat)
            if axis == "runtime":
                # First column is already the runtime pole; drop the duplicate.
                row = [row[0]] + row[2:]
            md_rows.append(row)
            axes = (sc or {}).get("axes") or {}
            metrics = (sc or {}).get("metrics") or {}
            workers = []
            for key in ("ingress_workers", "policy_workers", "io_workers"):
                if key in axes:
                    workers.append(f"{key}={axes[key]}")
            csv_rows.append(
                [
                    mid,
                    axes.get("runtime", ""),
                    fmt(qps(sc)),
                    fmt(lat_avg(sc)),
                    metrics.get("queries_sent", ""),
                    metrics.get("queries_completed", ""),
                    metrics.get("queries_lost", ""),
                    ", ".join(workers),
                ]
            )

    chart_id = figure_id
    if all(v is None for v in values):
        return ChartSpec(
            id=chart_id,
            title=title,
            y_label=y_label,
            categories=categories,
            series=[],
            unavailable_note=(
                f"_Study figure `{figure_id}` ({study_id}) unavailable — "
                "promoted reference lacks member results._"
            ),
        )

    if primary_metric == "drain_duration_ms":
        headers = [
            _axis_table_header(axis),
            "Drain duration (ms)",
            "Client failures during stop",
            "QPS",
            "Avg latency (ms)",
        ]
        csv_headers = [
            "scenario_id",
            "category",
            "drain_duration_ms",
            "client_failures_during_stop",
            "achieved_qps",
            "latency_avg_ms",
        ]
    else:
        headers = list(SCALE_TABLE_HEADERS)
        headers[0] = _axis_table_header(axis)
        if axis == "runtime":
            headers = [headers[0]] + headers[2:]
        csv_headers = SCALE_CSV_HEADERS

    return ChartSpec(
        id=chart_id,
        title=title,
        y_label=y_label,
        categories=categories,
        series=[(primary_metric, values)],
        table_headers=headers,
        table_rows=md_rows,
        csv_headers=csv_headers,
        csv_rows=csv_rows,
    )


def charts_for_studies(
    doc: dict[str, Any],
    studies: list[Any],
    *,
    link_scenarios: bool = True,
    published_only: bool = True,
) -> list[tuple[Any, list[ChartSpec]]]:
    """Return (study, charts) for each study with figure recipes."""
    smap = scenario_map(doc)
    out: list[tuple[Any, list[ChartSpec]]] = []
    for study in studies:
        if published_only and not getattr(study, "published", False):
            continue
        charts: list[ChartSpec] = []
        for fig in study.figures:
            charts.append(
                build_study_figure_chart(
                    study_id=study.id,
                    figure_id=fig.id,
                    title=fig.title,
                    y_label=fig.y_label,
                    member_ids=fig.members,
                    smap=smap,
                    primary_metric=study.primary_metric,
                    compare_axis=study.compare_axis,
                    category_axis=fig.category_axis,
                    link_scenarios=link_scenarios,
                )
            )
        out.append((study, charts))
    return out
