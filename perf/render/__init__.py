"""Renderers for performance run JSON (no re-bench)."""

from __future__ import annotations

import json
from typing import Any

try:
    import yaml
except ImportError as exc:  # pragma: no cover
    raise SystemExit(
        "PyYAML is required for perf.render (pip install -r perf/requirements.txt)"
    ) from exc


def render_json(doc: dict[str, Any]) -> str:
    return json.dumps(doc, indent=2, sort_keys=False) + "\n"


def render_yaml(doc: dict[str, Any]) -> str:
    return yaml.safe_dump(doc, sort_keys=False, default_flow_style=False)


def _latency_primary(latency_ms: dict[str, Any] | None) -> tuple[str, float | None]:
    """Pick the latency figure dnsperf currently provides (avg).

    Percentiles (p99, …) remain in the schema for when the loadgen path
    records them; render prefers avg until then.
    """
    lat = latency_ms or {}
    avg = lat.get("avg")
    if isinstance(avg, (int, float)):
        return "avg", float(avg)
    return "avg", None


def _scenario_lines(doc: dict[str, Any], *, fancy: bool) -> list[str]:
    check = "✓" if fancy else "OK"
    skip = "⊘" if fancy else "SKIP"
    err = "✗" if fancy else "ERR"
    lines: list[str] = []
    for sc in doc.get("scenarios", []):
        status = sc.get("status", "?")
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
            otlp_s = f"otlp_accepts={otlp_a if isinstance(otlp_a, int) else '-'} otlp_failures={otlp_f if isinstance(otlp_f, int) else '-'}"
        axes = sc.get("axes") or {}
        axis_bits = []
        for key in ("runtime", "load_shape", "drain_policy", "obs_posture"):
            if key in axes:
                axis_bits.append(f"{key}={axes[key]}")
        axis_s = (" [" + ", ".join(axis_bits) + "]") if axis_bits else ""
        bits = [qps_s, lat_s, drain_s, cold_s, apply_s, loss_s, otlp_s]
        extra = "  " + " ".join(b for b in bits if b)
        lines.append(f"  {mark} {sc.get('id')} ({sc.get('suite')}){axis_s}{extra.rstrip()}")
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
        *_scenario_lines(doc, fancy=False),
    ]
    run_anns = doc.get("annotation_ids") or []
    if run_anns:
        lines.append(f"run_annotations: {', '.join(run_anns)}")
    return "\n".join(lines) + "\n"


def render_fancy(doc: dict[str, Any]) -> str:
    profile = doc.get("lab_profile") or {}
    prov = doc.get("provenance") or {}
    lines = [
        "══ DNSConduit performance run ══",
        f"⏱  {doc.get('generated_at')}",
        f"🖥  {profile.get('id')} — {profile.get('display_name')}",
        f"   CPU {profile.get('cpu_model')}  "
        f"({profile.get('physical_cores')}p/{profile.get('logical_cores')}l)",
        *(
            [f"   RAM {profile.get('memory_total_mb')} MiB"]
            if profile.get("memory_total_mb") is not None
            else []
        ),
        f"📦  {prov.get('conduit_version')} @ {prov.get('conduit_path')}",
        "── scenarios ──",
        *_scenario_lines(doc, fancy=True),
    ]
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
        rows.append(
            "<tr>"
            f"<td>{_esc(sc.get('id'))}</td>"
            f"<td>{_esc(sc.get('suite'))}</td>"
            f"<td>{_esc(sc.get('status'))}</td>"
            f"<td>{_esc(metrics.get('achieved_qps'))}</td>"
            f"<td>{_esc(lat_cell)}</td>"
            f"<td>{_esc(drain_cell)}</td>"
            f"<td>{_esc(loss)}</td>"
            f"<td>{_esc(otlp_a)}</td>"
            f"<td>{_esc(otlp_f)}</td>"
            f"<td>{_esc(cold_cell)}</td>"
            f"<td>{_esc(apply_cell)}</td>"
            f"<td>{_esc(anns)}</td>"
            "</tr>"
        )
    body_rows = "\n".join(rows) if rows else "<tr><td colspan='12'>No scenarios</td></tr>"
    mem = profile.get("memory_total_mb")
    mem_html = (
        f"    <p><strong>Memory:</strong> {_esc(mem)} MiB</p>\n" if mem is not None else ""
    )
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <title>DNSConduit performance run</title>
  <style>
    body {{ font-family: system-ui, sans-serif; margin: 2rem; }}
    table {{ border-collapse: collapse; width: 100%; }}
    th, td {{ border: 1px solid #ccc; padding: 0.4rem 0.6rem; text-align: left; }}
    th {{ background: #f4f4f4; }}
    .meta {{ margin-bottom: 1.5rem; }}
  </style>
</head>
<body>
  <h1>DNSConduit performance run</h1>
  <div class="meta">
    <p><strong>Generated:</strong> {_esc(doc.get('generated_at'))}</p>
    <p><strong>Lab profile:</strong> {_esc(profile.get('id'))}
       — {_esc(profile.get('display_name'))}</p>
    <p><strong>CPU:</strong> {_esc(profile.get('cpu_model'))}</p>
{mem_html}    <p><strong>Conduit:</strong> {_esc(prov.get('conduit_version'))}
       ({_esc(prov.get('conduit_path'))})</p>
  </div>
  <table>
    <thead>
      <tr>
        <th>Scenario</th><th>Suite</th><th>Status</th>
        <th>Achieved QPS</th><th>avg ms</th>
        <th>Drain ms</th><th>Loss at stop</th>
        <th>OTLP accepts</th><th>OTLP failures</th>
        <th>Cold start ms</th><th>Apply ms</th><th>Annotations</th>
      </tr>
    </thead>
    <tbody>
      {body_rows}
    </tbody>
  </table>
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
    "fancy": render_fancy,
    "yaml": render_yaml,
    "json": render_json,
    "html": render_html,
}


def render(doc: dict[str, Any], fmt: str) -> str:
    if fmt not in FORMATS:
        raise ValueError(f"unknown format {fmt!r}; choose from {sorted(FORMATS)}")
    return FORMATS[fmt](doc)
