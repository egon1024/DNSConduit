"""Renderers for performance run JSON (no re-bench)."""

from __future__ import annotations

import json
import os
import sys
from typing import Any

try:
    import yaml
except ImportError as exc:  # pragma: no cover
    raise SystemExit(
        "PyYAML is required for perf.render (pip install -r perf/requirements.txt)"
    ) from exc

from .charts import (
    ChartSpec,
    charts_for_document,
    side_by_side_panels,
    svg_grouped_bars,
    unicode_bars,
    visible_width,
)


def render_json(doc: dict[str, Any]) -> str:
    return json.dumps(doc, indent=2, sort_keys=False) + "\n"


def render_yaml(doc: dict[str, Any]) -> str:
    return yaml.safe_dump(doc, sort_keys=False, default_flow_style=False)


def _latency_primary(latency_ms: dict[str, Any] | None) -> tuple[str, float | None]:
    """Pick the latency figure dnsperf currently provides (avg)."""
    lat = latency_ms or {}
    avg = lat.get("avg")
    if isinstance(avg, (int, float)):
        return "avg", float(avg)
    return "avg", None


def _use_color(*, force: bool | None = None) -> bool:
    if force is not None:
        return force
    if os.environ.get("NO_COLOR"):
        return False
    if os.environ.get("FORCE_COLOR"):
        return True
    return sys.stdout.isatty()


class _Ansi:
    def __init__(self, enabled: bool) -> None:
        self.enabled = enabled

    def wrap(self, code: str, text: str) -> str:
        if not self.enabled:
            return text
        return f"\033[{code}m{text}\033[0m"

    def bold(self, text: str) -> str:
        return self.wrap("1", text)

    def dim(self, text: str) -> str:
        return self.wrap("2", text)

    def green(self, text: str) -> str:
        return self.wrap("32", text)

    def yellow(self, text: str) -> str:
        return self.wrap("33", text)

    def red(self, text: str) -> str:
        return self.wrap("31", text)

    def cyan(self, text: str) -> str:
        return self.wrap("36", text)

    def magenta(self, text: str) -> str:
        return self.wrap("35", text)


def _scenario_lines(doc: dict[str, Any], *, rich: bool, ansi: _Ansi | None = None) -> list[str]:
    check = "✓" if rich else "OK"
    skip = "⊘" if rich else "SKIP"
    err = "✗" if rich else "ERR"
    color = ansi or _Ansi(False)
    lines: list[str] = []
    for sc in doc.get("scenarios", []):
        status = sc.get("status", "?")
        if rich:
            if status == "ok":
                mark = color.green(check)
            elif status == "skip":
                mark = color.yellow(skip)
            else:
                mark = color.red(err)
        else:
            mark = check if status == "ok" else skip if status == "skip" else err
        metrics = sc.get("metrics") or {}
        qps = metrics.get("achieved_qps")
        qps_s = f"{qps:.1f} qps" if isinstance(qps, (int, float)) else "-"
        lat_label, lat = _latency_primary(metrics.get("latency_ms"))
        lat_s = f"{lat_label}={lat:.2f}ms" if lat is not None else ""
        drain = metrics.get("drain_duration_ms")
        drain_s = (
            f"drain={drain:.1f}ms" if isinstance(drain, (int, float)) else ""
        )
        cold = metrics.get("cold_start_ms")
        cold_s = (
            f"cold_start={cold:.1f}ms" if isinstance(cold, (int, float)) else ""
        )
        apply = metrics.get("apply_latency_ms")
        apply_s = (
            f"apply={apply:.1f}ms" if isinstance(apply, (int, float)) else ""
        )
        secondary = sc.get("secondary") or {}
        loss = secondary.get("client_failures_during_stop")
        loss_s = f"loss={loss}" if isinstance(loss, int) else ""
        otlp_a = secondary.get("otlp_accepts")
        otlp_f = secondary.get("otlp_failures")
        otlp_s = ""
        if isinstance(otlp_a, int) or isinstance(otlp_f, int):
            otlp_s = (
                f"otlp_accepts={otlp_a if isinstance(otlp_a, int) else '-'} "
                f"otlp_failures={otlp_f if isinstance(otlp_f, int) else '-'}"
            )
        sent = metrics.get("queries_sent")
        completed = metrics.get("queries_completed")
        lost = metrics.get("queries_lost")
        traffic_s = ""
        if isinstance(sent, (int, float)) or isinstance(completed, (int, float)):
            traffic_s = (
                f"sent={sent if isinstance(sent, (int, float)) else '-'} "
                f"ok={completed if isinstance(completed, (int, float)) else '-'} "
                f"lost={lost if isinstance(lost, (int, float)) else '-'}"
            )
        axes = sc.get("axes") or {}
        axis_bits = []
        for key in ("runtime", "load_shape", "drain_policy", "obs_posture"):
            if key in axes:
                axis_bits.append(f"{key}={axes[key]}")
        axis_s = (" [" + ", ".join(axis_bits) + "]") if axis_bits else ""
        sid = sc.get("id")
        sid_s = color.cyan(str(sid)) if rich else str(sid)
        bits = [qps_s, lat_s, drain_s, cold_s, apply_s, loss_s, otlp_s, traffic_s]
        extra = "  " + " ".join(b for b in bits if b)
        lines.append(
            f"  {mark} {sid_s} ({sc.get('suite')}){axis_s}{extra.rstrip()}"
        )
        if status == "skip" and sc.get("skip_reason"):
            lines.append(f"      skip: {sc['skip_reason']}")
        if status == "error" and sc.get("error"):
            lines.append(f"      error: {sc['error']}")
        anns = sc.get("annotation_ids") or []
        if anns:
            lines.append(f"      annotations: {', '.join(anns)}")
    return lines


def render_plain(doc: dict[str, Any]) -> str:
    profile = doc.get("lab_profile") or {}
    prov = doc.get("provenance") or {}
    lines = [
        "DNSConduit performance run",
        f"generated_at: {doc.get('generated_at')}",
        f"lab_profile: {profile.get('id')} ({profile.get('display_name')})",
        f"cpu: {profile.get('cpu_model')}",
        f"cores: physical={profile.get('physical_cores')} logical={profile.get('logical_cores')}",
        f"os: {profile.get('os')}",
        *(
            [f"kernel: {profile.get('kernel')}"]
            if profile.get("kernel")
            else []
        ),
        *(
            [f"memory_total_mb: {profile.get('memory_total_mb')}"]
            if profile.get("memory_total_mb") is not None
            else []
        ),
        f"conduit: {prov.get('conduit_path')} ({prov.get('conduit_version')})",
        f"loadgen: {((prov.get('loadgen') or {}).get('tool'))} "
        f"mode={((prov.get('loadgen') or {}).get('mode'))}",
        "scenarios:",
        *_scenario_lines(doc, rich=False),
    ]
    run_anns = doc.get("annotation_ids") or []
    if run_anns:
        lines.append(f"run_annotations: {', '.join(run_anns)}")
    return "\n".join(lines) + "\n"


def render_rich(doc: dict[str, Any], *, color: bool | None = None) -> str:
    ansi = _Ansi(_use_color(force=color))
    profile = doc.get("lab_profile") or {}
    prov = doc.get("provenance") or {}
    lines = [
        ansi.bold(ansi.magenta("══ DNSConduit performance run ══")),
        f"{ansi.dim('⏱')}  {doc.get('generated_at')}",
        f"{ansi.dim('🖥')}  {profile.get('id')} — {profile.get('display_name')}",
        f"   CPU {profile.get('cpu_model')}  "
        f"({profile.get('physical_cores')}p/{profile.get('logical_cores')}l)",
        *(
            [f"   RAM {profile.get('memory_total_mb')} MiB"]
            if profile.get("memory_total_mb") is not None
            else []
        ),
        f"{ansi.dim('📦')}  {prov.get('conduit_version')} @ {prov.get('conduit_path')}",
        ansi.bold("── scenarios ──"),
        *_scenario_lines(doc, rich=True, ansi=ansi),
    ]

    def _panel(chart: ChartSpec) -> list[str]:
        short = chart.title
        # Prefer compact titles when pairing side-by-side.
        for prefix in (
            "Achieved QPS — ",
            "Feature tax — ",
            "Lifecycle — ",
        ):
            if short.startswith(prefix):
                short = short[len(prefix) :]
                break
        header = ansi.bold(ansi.cyan(f"── {short} ──"))
        bar_lines = unicode_bars(
            title="",
            categories=chart.categories,
            series=chart.series,
            height=10,
        )
        if ansi.enabled:
            bar_lines = [
                row.replace("█", ansi.magenta("█")).replace("▄", ansi.magenta("▄"))
                for row in bar_lines
            ]
        return [header, *bar_lines]

    # Pair related charts horizontally when both exist (uses wide terminal space).
    pair_ids = [
        (
            "scale-sync-vs-split-io-forward-fast",
            "scale-sync-vs-split-io-forward-slow",
        ),
        ("scale-cache-hit", "scale-topology-heavy"),
        ("feature-tax-metrics-scrape", "feature-tax-dnstap"),
        ("lifecycle-cold-start", "lifecycle-config-apply"),
    ]
    by_id = {
        c.id: c for c in charts_for_document(doc, link_scenarios=False) if c.has_data
    }
    used: set[str] = set()
    chart_lines: list[str] = []

    term_cols = 120
    try:
        import shutil

        env_cols = os.environ.get("COLUMNS")
        if env_cols and env_cols.isdigit():
            term_cols = int(env_cols)
        else:
            term_cols = shutil.get_terminal_size(fallback=(120, 40)).columns
    except OSError:
        pass

    for left_id, right_id in pair_ids:
        left = by_id.get(left_id)
        right = by_id.get(right_id)
        if left is None and right is None:
            continue
        if left is not None:
            used.add(left_id)
        if right is not None:
            used.add(right_id)
        chart_lines.append("")
        if left is not None and right is not None:
            lp = _panel(left)
            rp = _panel(right)
            plot_w = (
                max(visible_width(x) for x in lp)
                + 4
                + max(visible_width(x) for x in rp)
            )
            if plot_w <= max(term_cols - 2, 100):
                chart_lines.extend(side_by_side_panels(lp, rp, gap=4))
            else:
                chart_lines.extend(lp)
                chart_lines.append("")
                chart_lines.extend(rp)
        elif left is not None:
            chart_lines.extend(_panel(left))
        else:
            assert right is not None
            chart_lines.extend(_panel(right))

    for chart in by_id.values():
        if chart.id in used:
            continue
        chart_lines.append("")
        chart_lines.extend(_panel(chart))

    if chart_lines:
        lines.append("")
        lines.append(ansi.bold("── charts ──"))
        lines.extend(chart_lines)
    return "\n".join(lines) + "\n"


def render_html(doc: dict[str, Any]) -> str:
    profile = doc.get("lab_profile") or {}
    prov = doc.get("provenance") or {}
    rows = []
    for sc in doc.get("scenarios", []):
        metrics = sc.get("metrics") or {}
        secondary = sc.get("secondary") or {}
        _, lat_avg = _latency_primary(metrics.get("latency_ms"))
        anns = ", ".join(sc.get("annotation_ids") or [])
        lat_cell = f"{lat_avg:.3f}" if lat_avg is not None else ""
        drain = metrics.get("drain_duration_ms")
        drain_cell = f"{drain:.1f}" if isinstance(drain, (int, float)) else ""
        loss = secondary.get("client_failures_during_stop")
        otlp_a = secondary.get("otlp_accepts")
        otlp_f = secondary.get("otlp_failures")
        cold = metrics.get("cold_start_ms")
        cold_cell = f"{cold:.1f}" if isinstance(cold, (int, float)) else ""
        apply = metrics.get("apply_latency_ms")
        apply_cell = f"{apply:.1f}" if isinstance(apply, (int, float)) else ""
        status = sc.get("status")
        status_cls = (
            "ok" if status == "ok" else "skip" if status == "skip" else "err"
        )
        rows.append(
            "<tr>"
            f"<td>{_esc(sc.get('id'))}</td>"
            f"<td>{_esc(sc.get('suite'))}</td>"
            f'<td class="{status_cls}">{_esc(status)}</td>'
            f"<td>{_esc(metrics.get('achieved_qps'))}</td>"
            f"<td>{_esc(lat_cell)}</td>"
            f"<td>{_esc(metrics.get('queries_sent'))}</td>"
            f"<td>{_esc(metrics.get('queries_completed'))}</td>"
            f"<td>{_esc(metrics.get('queries_lost'))}</td>"
            f"<td>{_esc(drain_cell)}</td>"
            f"<td>{_esc(loss)}</td>"
            f"<td>{_esc(otlp_a)}</td>"
            f"<td>{_esc(otlp_f)}</td>"
            f"<td>{_esc(cold_cell)}</td>"
            f"<td>{_esc(apply_cell)}</td>"
            f"<td>{_esc(anns)}</td>"
            "</tr>"
        )
    body_rows = (
        "\n".join(rows) if rows else "<tr><td colspan='15'>No scenarios</td></tr>"
    )
    mem = profile.get("memory_total_mb")
    mem_html = (
        f"    <p><strong>Memory:</strong> {_esc(mem)} MiB</p>\n" if mem is not None else ""
    )
    chart_blocks: list[str] = []
    for chart in charts_for_document(doc, link_scenarios=False):
        if not chart.has_data:
            continue
        svg = svg_grouped_bars(
            title=chart.title,
            categories=chart.categories,
            series=chart.series,
            y_label=chart.y_label,
        )
        chart_blocks.append(
            f'<section class="chart"><h2>{_esc(chart.title)}</h2>\n{svg}</section>'
        )
    charts_html = "\n".join(chart_blocks)
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <title>DNSConduit performance run</title>
  <style>
    :root {{
      --bg: #f7f8fb;
      --card: #ffffff;
      --ink: #1f2933;
      --muted: #52606d;
      --line: #d9e2ec;
      --ok: #0f7b3c;
      --skip: #9a6700;
      --err: #b00020;
      --accent: #3949ab;
    }}
    body {{
      font-family: "Segoe UI", system-ui, sans-serif;
      margin: 0;
      color: var(--ink);
      background: linear-gradient(180deg, #eef2ff 0%, var(--bg) 280px);
    }}
    main {{ max-width: 1100px; margin: 0 auto; padding: 2rem 1.25rem 3rem; }}
    h1 {{ color: var(--accent); margin-bottom: 0.35rem; }}
    .meta {{
      background: var(--card);
      border: 1px solid var(--line);
      border-radius: 10px;
      padding: 1rem 1.25rem;
      margin-bottom: 1.5rem;
      color: var(--muted);
    }}
    .meta p {{ margin: 0.35rem 0; }}
    table {{
      border-collapse: collapse;
      width: 100%;
      background: var(--card);
      border: 1px solid var(--line);
      border-radius: 10px;
      overflow: hidden;
      font-size: 0.92rem;
    }}
    th, td {{ border-bottom: 1px solid var(--line); padding: 0.45rem 0.6rem; text-align: left; }}
    th {{ background: #eef2ff; }}
    td.ok {{ color: var(--ok); font-weight: 600; }}
    td.skip {{ color: var(--skip); font-weight: 600; }}
    td.err {{ color: var(--err); font-weight: 600; }}
    .chart {{
      margin: 1.5rem 0;
      background: var(--card);
      border: 1px solid var(--line);
      border-radius: 10px;
      padding: 1rem;
    }}
    .chart h2 {{ margin-top: 0; font-size: 1.05rem; }}
    .chart svg {{ max-width: 100%; height: auto; }}
  </style>
</head>
<body>
<main>
  <h1>DNSConduit performance run</h1>
  <div class="meta">
    <p><strong>Generated:</strong> {_esc(doc.get('generated_at'))}</p>
    <p><strong>Lab profile:</strong> {_esc(profile.get('id'))}
       — {_esc(profile.get('display_name'))}</p>
    <p><strong>CPU:</strong> {_esc(profile.get('cpu_model'))}</p>
{mem_html}    <p><strong>Conduit:</strong> {_esc(prov.get('conduit_version'))}
       ({_esc(prov.get('conduit_path'))})</p>
  </div>
  {charts_html}
  <h2>Scenario table</h2>
  <table>
    <thead>
      <tr>
        <th>Scenario</th><th>Suite</th><th>Status</th>
        <th>Achieved QPS</th><th>avg ms</th>
        <th>Sent</th><th>Completed</th><th>Lost</th>
        <th>Drain ms</th><th>Loss at stop</th>
        <th>OTLP accepts</th><th>OTLP failures</th>
        <th>Cold start ms</th><th>Apply ms</th><th>Annotations</th>
      </tr>
    </thead>
    <tbody>
      {body_rows}
    </tbody>
  </table>
</main>
</body>
</html>
"""


def _esc(value: Any) -> str:
    if value is None:
        return ""
    text = str(value)
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


FORMATS = {
    "plain": render_plain,
    "rich": render_rich,
    "yaml": render_yaml,
    "json": render_json,
    "html": render_html,
}


def render(doc: dict[str, Any], fmt: str) -> str:
    if fmt not in FORMATS:
        raise ValueError(f"unknown format {fmt!r}; choose from {sorted(FORMATS)}")
    return FORMATS[fmt](doc)
