"""Promote run JSON to curated references and generate operator-docs fragments."""

from __future__ import annotations

import csv
import io
import re
import statistics
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    import yaml
except ImportError as exc:  # pragma: no cover
    raise SystemExit(
        "PyYAML is required for perf.runner (pip install -r perf/requirements.txt)"
    ) from exc

from perf.render.charts import (
    CURATED_SCENARIO_IDS,
    ChartSpec,
    charts_for_document,
    charts_for_studies,
    md_table,
    svg_grouped_bars,
)

from .catalog import (
    Annotation,
    Study,
    load_annotations,
    load_scenarios,
    load_studies,
    publish_set_member_ids,
)
from .integrity import (
    TakeawayIntegrityError,
    claims_from_charts,
    format_delta_fragment,
    verify_studies_integrity,
)
from .paths import REFERENCES_DIR, ROOT, load_json, write_json
from .run_record import validate_run_document, utc_now_iso

OPERATOR_PERF = ROOT / "operator-docs" / "docs" / "performance"
GENERATED_DIR = OPERATOR_PERF / "generated"
LATEST_POINTER = REFERENCES_DIR / "latest-reference.json"

STUDY_EVIDENCE_START = "<!-- perf-study-evidence:start -->"
STUDY_EVIDENCE_END = "<!-- perf-study-evidence:end -->"
STUDY_DELTAS_START = "<!-- perf-study-deltas:start -->"
STUDY_DELTAS_END = "<!-- perf-study-deltas:end -->"
STUDIES_INDEX_START = "<!-- perf-studies-index:start -->"
STUDIES_INDEX_END = "<!-- perf-studies-index:end -->"
REFERENCE_BODY_START = "<!-- perf-reference-body:start -->"
REFERENCE_BODY_END = "<!-- perf-reference-body:end -->"
SCENARIOS_BODY_START = "<!-- perf-scenarios-body:start -->"
SCENARIOS_BODY_END = "<!-- perf-scenarios-body:end -->"
# Per-annotation include markers: <!-- perf-ann:<id>:start --> … <!-- perf-ann:<id>:end -->
_ANN_INCLUDE_START_RE = re.compile(
    r"<!--\s*perf-ann:([a-zA-Z0-9_-]+):start\s*-->"
)


def _studies_docs_dir() -> Path:
    return OPERATOR_PERF / "studies"


def _inject_marked_section(
    page_text: str,
    *,
    start_marker: str,
    end_marker: str,
    body: str,
) -> str:
    """Replace content between markers; append markers+body if missing."""
    start = page_text.find(start_marker)
    end = page_text.find(end_marker)
    block = f"{start_marker}\n{body.rstrip()}\n{end_marker}"
    if start == -1 or end == -1 or end < start:
        return page_text.rstrip() + "\n\n" + block + "\n"
    return page_text[:start] + block + page_text[end + len(end_marker) :]


def _includes_dir() -> Path:
    return OPERATOR_PERF / "includes"


def _read_include(name: str, *, fallback: str) -> str:
    path = _includes_dir() / name
    if path.is_file():
        return path.read_text(encoding="utf-8").rstrip() + "\n"
    return fallback.rstrip() + "\n"


def _ensure_page_shell(path: Path, shell: str) -> None:
    """Create a hand-authored page shell once if missing (tests / fresh trees)."""
    if not path.is_file():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(shell, encoding="utf-8")

# Back-compat aliases for tests / callers.
THIN_SCALE_IDS = (
    "scale-sync-forward-fast",
    "scale-sync-forward-slow",
    "scale-split-io-forward-fast",
    "scale-split-io-forward-slow",
)
THIN_DRAIN_IDS = (
    "shutdown-drain-complete-forward-slow",
    "shutdown-drain-budgeted-forward-slow",
    "shutdown-drain-minimal-forward-slow",
)


def publish_set_scenario_ids() -> list[str]:
    """Scenario ids required by published studies (ordered union)."""
    scenarios = load_scenarios()
    studies = load_studies(scenarios=scenarios)
    return publish_set_member_ids(studies)


def merge_run_documents(docs: list[dict[str, Any]]) -> dict[str, Any]:
    """Merge scenario lists from multiple runs; later docs win on duplicate ids."""
    if not docs:
        raise ValueError("no run documents to merge")
    base = dict(docs[0])
    by_id: dict[str, dict[str, Any]] = {}
    run_anns: list[str] = []
    for doc in docs:
        for aid in doc.get("annotation_ids") or []:
            if aid not in run_anns:
                run_anns.append(aid)
        for sc in doc.get("scenarios") or []:
            by_id[sc["id"]] = sc
        # Prefer the last document's lab_profile / provenance for promote.
        base["lab_profile"] = doc["lab_profile"]
        base["provenance"] = doc["provenance"]
        if "quality" in doc:
            base["quality"] = doc["quality"]
    base["scenarios"] = list(by_id.values())
    base["generated_at"] = utc_now_iso()
    if run_anns:
        base["annotation_ids"] = run_anns
    return base


def _median_merge_values(key: str, values: list[Any]) -> Any:
    """Median-merge one field's per-round values; recurse into nested dicts."""
    present = [v for v in values if v is not None]
    if not present:
        return None
    if isinstance(present[0], dict):
        keys: list[str] = []
        for v in present:
            for k in v:
                if k not in keys:
                    keys.append(k)
        return {k: _median_merge_values(k, [v.get(k) for v in present]) for k in keys}
    numeric = [v for v in present if isinstance(v, (int, float)) and not isinstance(v, bool)]
    if len(numeric) == len(present) and numeric:
        med = statistics.median(numeric)
        if all(isinstance(v, int) and not isinstance(v, bool) for v in present):
            return int(round(med))
        return med
    # Non-numeric (or mixed) fields: last round wins.
    return present[-1]


def merge_median_documents(docs: list[dict[str, Any]]) -> dict[str, Any]:
    """Combine N same-shape round documents into one run via per-field median.

    Each *doc* is a full round produced by an identical scenario selection
    (e.g. three separate ``perf.runner run --suite feature_tax`` invocations).
    For each scenario id, numeric fields under ``metrics``/``secondary`` are
    the median across rounds with ``status: ok``; non-numeric fields and
    metadata (axes, intent, quality) come from the last ok round. A single
    outlier round therefore cannot swing the published number the way one
    single-shot run could.
    """
    if not docs:
        raise ValueError("no run documents to merge")
    if len(docs) < 2:
        raise ValueError("median merge requires at least 2 round documents")

    by_id: dict[str, list[dict[str, Any]]] = {}
    order: list[str] = []
    for doc in docs:
        for sc in doc.get("scenarios") or []:
            sid = sc["id"]
            if sid not in by_id:
                by_id[sid] = []
                order.append(sid)
            by_id[sid].append(sc)

    merged_scenarios: list[dict[str, Any]] = []
    for sid in order:
        rounds = by_id[sid]
        ok_rounds = [r for r in rounds if r.get("status") == "ok"]
        invalid_rounds = [r for r in rounds if r.get("status") == "invalid"]
        if not ok_rounds:
            merged_scenarios.append(rounds[-1])
            continue
        if invalid_rounds:
            # A cell that fails the answer gate in some rounds is sitting on the
            # boundary; a median across the surviving rounds would hide that.
            merged_scenarios.append(invalid_rounds[-1])
            continue
        rep = dict(ok_rounds[-1])
        rep["metrics"] = _median_merge_values(
            "metrics", [r.get("metrics") for r in ok_rounds]
        )
        secondary_vals = [r.get("secondary") for r in ok_rounds if r.get("secondary")]
        if secondary_vals:
            rep["secondary"] = _median_merge_values("secondary", secondary_vals)
        qps_values = [
            (r.get("metrics") or {}).get("achieved_qps")
            for r in ok_rounds
            if (r.get("metrics") or {}).get("achieved_qps") is not None
        ]
        quality = dict(rep.get("quality") or {})
        if qps_values:
            note = (
                f"median of {len(ok_rounds)} rounds; achieved_qps range "
                f"{min(qps_values):.1f}-{max(qps_values):.1f}"
            )
            existing_note = quality.get("notes")
            quality["notes"] = f"{existing_note}; {note}" if existing_note else note
        rep["quality"] = quality
        merged_scenarios.append(rep)

    base = dict(docs[-1])
    base["scenarios"] = merged_scenarios
    base["generated_at"] = utc_now_iso()
    run_anns: list[str] = []
    for doc in docs:
        for aid in doc.get("annotation_ids") or []:
            if aid not in run_anns:
                run_anns.append(aid)
    if run_anns:
        base["annotation_ids"] = run_anns
    return base


def assert_no_invalid_scenarios(doc: dict[str, Any]) -> None:
    """Refuse to promote measurements the harness already judged untrustworthy.

    A scenario is ``invalid`` when its answer gate failed — achieved QPS then
    reflects how fast Conduit rejected queries, not how many it served.
    """
    offenders = [
        sc for sc in doc.get("scenarios") or [] if sc.get("status") == "invalid"
    ]
    if not offenders:
        return
    detail = "; ".join(
        f"{sc.get('id')}: {sc.get('error') or 'answer gate failed'}"
        for sc in offenders
    )
    raise ValueError(
        f"refusing to promote {len(offenders)} invalid scenario(s) — {detail}"
    )


def promote_runs(
    sources: list[Path],
    *,
    name: str,
    annotation_ids: list[str] | None = None,
    profile_id: str | None = None,
    thin_spine: bool = False,
    publish_set: bool = False,
) -> Path:
    """Validate, optionally retarget profile id, write references/<name>.json + pointer."""
    docs = [load_json(p) for p in sources]
    merged = merge_run_documents(docs)
    if publish_set and thin_spine:
        raise ValueError("use only one of --publish-set or --thin-spine")
    if publish_set:
        keep = set(publish_set_scenario_ids())
        merged["scenarios"] = [
            sc for sc in merged.get("scenarios") or [] if sc.get("id") in keep
        ]
    elif thin_spine:
        keep = set(CURATED_SCENARIO_IDS)
        merged["scenarios"] = [
            sc for sc in merged.get("scenarios") or [] if sc.get("id") in keep
        ]
    if profile_id:
        merged["lab_profile"] = dict(merged["lab_profile"])
        merged["lab_profile"]["id"] = profile_id
        merged["lab_profile"]["display_name"] = (
            f"Maintainer workstation ({profile_id})"
        )
    if annotation_ids:
        existing = list(merged.get("annotation_ids") or [])
        for aid in annotation_ids:
            if aid not in existing:
                existing.append(aid)
        merged["annotation_ids"] = existing
    assert_no_invalid_scenarios(merged)
    validate_run_document(merged)
    REFERENCES_DIR.mkdir(parents=True, exist_ok=True)
    dest = REFERENCES_DIR / f"{name}.json"
    write_json(dest, merged)
    pointer = {
        "schema_version": 1,
        "updated_at": utc_now_iso(),
        "reference": dest.name,
        "path": str(dest.relative_to(ROOT)),
        "lab_profile_id": merged["lab_profile"]["id"],
        "scenario_count": len(merged.get("scenarios") or []),
    }
    write_json(LATEST_POINTER, pointer)
    return dest


def load_latest_reference() -> dict[str, Any] | None:
    if not LATEST_POINTER.is_file():
        candidates = sorted(REFERENCES_DIR.glob("thin-spine*.json"))
        if not candidates:
            return None
        return load_json(candidates[-1])
    pointer = load_json(LATEST_POINTER)
    ref_name = pointer.get("reference")
    if not ref_name:
        return None
    path = REFERENCES_DIR / ref_name
    if not path.is_file():
        return None
    return load_json(path)


def _write_csv(path: Path, headers: list[str], rows: list[list[Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    buf = io.StringIO()
    writer = csv.writer(buf)
    writer.writerow(headers)
    for row in rows:
        writer.writerow(row)
    path.write_text(buf.getvalue(), encoding="utf-8")


def _chart_fragment_md(chart: ChartSpec, *, rel_generated: str = "generated") -> str:
    """Markdown for one chart; wrapped in a perf-chart card (scenarios-page pattern)."""
    if chart.unavailable_note:
        inner = f"### {chart.title}\n\n{chart.unavailable_note}\n"
    else:
        svg_name = f"{chart.id}.svg"
        csv_name = f"{chart.id}.csv"
        table = ""
        if chart.table_headers and chart.table_rows:
            # Replace empty cells with em dash for display.
            display_rows = [
                [c if c not in ("", None) else "—" for c in row]
                for row in chart.table_rows
            ]
            table = md_table(chart.table_headers, display_rows)
        inner = (
            f"### {chart.title}\n\n"
            f"![{chart.title}]({rel_generated}/{svg_name})\n\n"
            f"[Download CSV]({rel_generated}/{csv_name})\n\n"
            f"{table}"
        )
    return (
        '<div class="perf-chart" markdown="1">\n\n'
        f"{inner.rstrip()}\n\n"
        "</div>\n"
    )


def _reference_chart_section(fragment_name: str) -> str:
    """Map chart-*.fragment.md basename to a suite-style H2 for the reference warehouse."""
    # chart-<id>.fragment.md → <id>
    stem = fragment_name
    if stem.startswith("chart-") and stem.endswith(".fragment.md"):
        stem = stem[len("chart-") : -len(".fragment.md")]
    if stem.startswith("scale-"):
        return "Scale"
    if stem.startswith("shutdown-"):
        return "Shutdown drain"
    if stem.startswith("feature-tax-"):
        return "Feature tax"
    if stem.startswith("lifecycle-"):
        return "Lifecycle"
    return "Charts"


def _write_chart_artifacts(
    chart: ChartSpec, *, fragment_prefix: str = "chart"
) -> list[Path]:
    written: list[Path] = []
    if chart.unavailable_note:
        note = GENERATED_DIR / f"{fragment_prefix}-{chart.id}.fragment.md"
        note.write_text(
            _chart_fragment_md(chart, rel_generated="generated"), encoding="utf-8"
        )
        written.append(note)
        return written

    svg = svg_grouped_bars(
        title=chart.title,
        categories=chart.categories,
        series=chart.series,
        y_label=chart.y_label,
        width=560 if len(chart.categories) <= 3 else 640,
    )
    svg_path = GENERATED_DIR / f"{chart.id}.svg"
    svg_path.write_text(svg, encoding="utf-8")
    written.append(svg_path)

    if chart.csv_headers:
        csv_path = GENERATED_DIR / f"{chart.id}.csv"
        _write_csv(csv_path, chart.csv_headers, chart.csv_rows)
        written.append(csv_path)

    md_path = GENERATED_DIR / f"{fragment_prefix}-{chart.id}.fragment.md"
    md_path.write_text(
        _chart_fragment_md(chart, rel_generated="generated"), encoding="utf-8"
    )
    written.append(md_path)
    return written


def _study_evidence_markdown(charts: list[ChartSpec]) -> str:
    """Evidence block for study pages (paths relative to studies/).

    Figures appear in the order the study declares them; each study page leads
    with the figure its takeaway is built on.
    """
    parts: list[str] = ["## Evidence", ""]
    for chart in charts:
        parts.append(_chart_fragment_md(chart, rel_generated="../generated").rstrip())
        parts.append("")
    return "\n".join(parts).rstrip() + "\n"


def _default_study_page(study: Study) -> str:
    guides = "\n".join(f"- [{g}]({g})" for g in study.related_guides) or "_None listed._"
    members = "\n".join(
        f"- [{mid}](/performance/scenarios.md#{mid})" for mid in study.members
    )
    disclaimer = _read_include(
        "same-host-disclaimer.fragment.md",
        fallback=(
            "Numbers are same-host comparisons (relative to baselines measured on one "
            "named lab profile) and are **not** service-level objectives. See the "
            "[performance hub disclaimer](/performance/index.md)."
        ),
    )
    return (
        f"# {study.title}\n\n"
        f"{study.question}\n\n"
        f"{disclaimer}\n"
        "## When this matters\n\n"
        f"{study.takeaway or '_Takeaway pending._'}\n\n"
        "## What we varied\n\n"
        f"Primary compare axis: `{study.compare_axis}`. "
        f"Primary metric: `{study.primary_metric}`.\n\n"
        f"{STUDY_EVIDENCE_START}\n"
        "_Evidence is generated from committed reference JSON "
        "(run `make perf-docs`)._\n"
        f"{STUDY_EVIDENCE_END}\n\n"
        f"{STUDY_DELTAS_START}\n"
        "## At a glance\n\n"
        "_Summary from the evidence tables (run `make perf-docs`)._\n"
        f"{STUDY_DELTAS_END}\n\n"
        "## Takeaway\n\n"
        f"{study.takeaway or '_See evidence above._'}\n\n"
        "## Related guides\n\n"
        f"{guides}\n\n"
        "## Member scenarios\n\n"
        f"{members}\n\n"
        "## Related\n\n"
        "- [Studies hub](/performance/studies/index.md)\n"
        "- [Reference results](/performance/reference.md)\n"
        "- [Methodology](/performance/methodology.md)\n"
    )


def _inject_study_evidence(page_text: str, evidence: str) -> str:
    return _inject_marked_section(
        page_text,
        start_marker=STUDY_EVIDENCE_START,
        end_marker=STUDY_EVIDENCE_END,
        body=evidence,
    )


def _inject_study_deltas(page_text: str, deltas: str) -> str:
    """Inject generated deltas above Takeaway when markers are absent."""
    start = page_text.find(STUDY_DELTAS_START)
    end = page_text.find(STUDY_DELTAS_END)
    block = f"{STUDY_DELTAS_START}\n{deltas.rstrip()}\n{STUDY_DELTAS_END}"
    if start != -1 and end != -1 and end > start:
        return page_text[:start] + block + page_text[end + len(STUDY_DELTAS_END) :]
    takeaway_at = page_text.find("## Takeaway")
    if takeaway_at != -1:
        return page_text[:takeaway_at] + block + "\n\n" + page_text[takeaway_at:]
    return page_text.rstrip() + "\n\n" + block + "\n"


def _studies_index_table_markdown(published: list[Study], *, stamp: str) -> str:
    lines = [
        f"_Generated index {stamp} from the study catalog "
        "(evidence from committed reference JSON)._",
        "",
        "| Study | Question |",
        "| --- | --- |",
    ]
    for study in _order_studies_like_nav(published):
        lines.append(
            f"| [{study.title}](/performance/studies/{study.id}.md) | {study.question} |"
        )
    return "\n".join(lines) + "\n"


def _load_mkdocs_yaml(path: Path) -> Any:
    """Load mkdocs.yml, ignoring !!python/* tags used by markdown extensions."""

    class _IgnorePythonTags(yaml.SafeLoader):
        pass

    def _ignore(loader: yaml.SafeLoader, tag_suffix: str, node: Any) -> None:
        return None

    _IgnorePythonTags.add_multi_constructor(
        "tag:yaml.org,2002:python/", _ignore
    )
    with path.open(encoding="utf-8") as f:
        return yaml.load(f, Loader=_IgnorePythonTags)


def _study_nav_order_ids() -> list[str]:
    """Study page ids in MkDocs Performance → Tuning evidence nav order."""
    mkdocs_path = ROOT / "operator-docs" / "mkdocs.yml"
    if not mkdocs_path.is_file():
        return []
    raw = _load_mkdocs_yaml(mkdocs_path)
    if not isinstance(raw, dict):
        return []
    nav = raw.get("nav")
    if not isinstance(nav, list):
        return []

    def walk(nodes: list[Any]) -> list[str] | None:
        for node in nodes:
            if isinstance(node, dict):
                for key, val in node.items():
                    if key == "Tuning evidence (studies)" and isinstance(val, list):
                        ids: list[str] = []
                        for item in val:
                            if not isinstance(item, dict):
                                continue
                            for _label, path in item.items():
                                if not isinstance(path, str):
                                    continue
                                if path == "performance/studies/index.md":
                                    continue
                                prefix = "performance/studies/"
                                suffix = ".md"
                                if path.startswith(prefix) and path.endswith(suffix):
                                    ids.append(path[len(prefix) : -len(suffix)])
                        return ids
                    if isinstance(val, list):
                        found = walk(val)
                        if found is not None:
                            return found
        return None

    return walk(nav) or []


def _order_studies_like_nav(studies: list[Study]) -> list[Study]:
    """Match studies hub table order to MkDocs nav (unknown ids last, by id)."""
    order = _study_nav_order_ids()
    rank = {sid: i for i, sid in enumerate(order)}

    def key(s: Study) -> tuple[int, str]:
        return (rank.get(s.id, 10_000), s.id)

    return sorted(studies, key=key)


STUDIES_INDEX_SHELL = f"""# Tuning evidence (studies)

{STUDIES_INDEX_START}
_Study index is generated from the catalog (run `make perf-docs`)._
{STUDIES_INDEX_END}
"""


def _write_studies_docs(
    doc: dict[str, Any] | None,
    *,
    stamp: str,
    check_integrity: bool = True,
) -> list[Path]:
    written: list[Path] = []
    studies_docs = _studies_docs_dir()
    studies_docs.mkdir(parents=True, exist_ok=True)
    scenarios = load_scenarios()
    try:
        studies = load_studies(scenarios=scenarios)
    except ValueError:
        studies = []

    published = [s for s in studies if s.published]
    index_path = studies_docs / "index.md"
    _ensure_page_shell(index_path, STUDIES_INDEX_SHELL)
    index_body = _studies_index_table_markdown(published, stamp=stamp)
    index_text = _inject_marked_section(
        index_path.read_text(encoding="utf-8"),
        start_marker=STUDIES_INDEX_START,
        end_marker=STUDIES_INDEX_END,
        body=index_body,
    )
    index_path.write_text(index_text, encoding="utf-8")
    written.append(index_path)

    if doc is None:
        for study in published:
            path = studies_docs / f"{study.id}.md"
            if not path.is_file():
                path.write_text(_default_study_page(study), encoding="utf-8")
                written.append(path)
            else:
                text = _inject_study_evidence(
                    path.read_text(encoding="utf-8"),
                    "## Evidence\n\n"
                    "_Reference measurements are not yet promoted; "
                    "figures unavailable._\n",
                )
                path.write_text(text, encoding="utf-8")
                written.append(path)
        return written

    studies_with_charts = charts_for_studies(doc, studies, published_only=True)
    page_text_by_id: dict[str, str] = {}
    for study, charts in studies_with_charts:
        for chart in charts:
            written.extend(_write_chart_artifacts(chart, fragment_prefix="study"))
        combined = GENERATED_DIR / f"study-{study.id}.fragment.md"
        combined.write_text(
            _study_evidence_markdown(charts).replace("../generated/", "generated/"),
            encoding="utf-8",
        )
        written.append(combined)

        claims = claims_from_charts(charts, primary_metric=study.primary_metric)
        deltas_md = format_delta_fragment(
            study_id=study.id,
            charts=charts,
            claims=claims,
            primary_metric=study.primary_metric,
        )
        deltas_path = GENERATED_DIR / f"study-{study.id}-deltas.fragment.md"
        deltas_path.write_text(deltas_md, encoding="utf-8")
        written.append(deltas_path)

        path = studies_docs / f"{study.id}.md"
        if path.is_file():
            body = path.read_text(encoding="utf-8")
        else:
            body = _default_study_page(study)
        body = _inject_study_evidence(body, _study_evidence_markdown(charts))
        body = _inject_study_deltas(body, deltas_md)
        path.write_text(body, encoding="utf-8")
        written.append(path)
        page_text_by_id[study.id] = body

    if check_integrity:
        errors = verify_studies_integrity(
            studies_with_charts, page_text_by_id=page_text_by_id
        )
        if errors:
            raise TakeawayIntegrityError(errors)

    return written


SCENARIOS_SHELL = f"""---
toc_depth: 3
toc_collapsible: true
---

# Performance scenarios

What each curated performance scenario measures. Rows on the
[reference results](/performance/reference.md) page link here.

{SCENARIOS_BODY_START}
_Scenario catalog body is generated (run `make perf-docs`)._
{SCENARIOS_BODY_END}
"""


def _scenarios_body_markdown(doc: dict[str, Any] | None) -> str:
    catalog = {s.id: s for s in load_scenarios()}
    ids: list[str] = []
    if doc:
        ids = [sc["id"] for sc in doc.get("scenarios") or []]
    for sid in CURATED_SCENARIO_IDS:
        if sid not in ids:
            ids.append(sid)
    try:
        for sid in publish_set_member_ids(load_studies(scenarios=list(catalog.values()))):
            if sid not in ids:
                ids.append(sid)
    except ValueError:
        pass
    lines: list[str] = []
    by_suite: dict[str, list[str]] = {}
    for sid in ids:
        sc = catalog.get(sid)
        suite = sc.suite if sc else "unknown"
        by_suite.setdefault(suite, []).append(sid)

    for suite in sorted(by_suite):
        lines.append(f"## {suite}")
        lines.append("")
        for sid in by_suite[suite]:
            sc = catalog.get(sid)
            # Card wrapper (md_in_html) so each scenario scans as a distinct block.
            lines.append('<div class="perf-scenario" markdown="1">')
            lines.append("")
            lines.append(f"### {sid}")
            lines.append("")
            if sc is None:
                lines.append("_Catalog entry missing._")
                lines.append("")
                lines.append("</div>")
                lines.append("")
                continue
            intent = sc.intent.strip() or "_No intent text in catalog._"
            lines.append(intent)
            lines.append("")
            if sc.axes:
                axis_bits = ", ".join(f"`{k}`=`{v}`" for k, v in sc.axes.items())
                lines.append(f"**Axes:** {axis_bits}")
                lines.append("")
            recipe = sc.recipe or {}
            if recipe.get("config") or recipe.get("load_shape") or recipe.get("upstream"):
                bits = []
                if recipe.get("config"):
                    bits.append(f"config `{recipe['config']}`")
                if recipe.get("upstream"):
                    bits.append(f"upstream `{recipe['upstream']}`")
                if recipe.get("loadgen"):
                    bits.append(f"loadgen `{recipe['loadgen']}`")
                if recipe.get("loadgen") == "dnsperf":
                    clients = recipe.get("clients", 4)
                    threads = recipe.get("dnsperf_threads", 2)
                    outstanding = recipe.get("max_outstanding")
                    concurrency = f"clients={clients}, threads={threads}"
                    if outstanding is not None:
                        concurrency += f", max_outstanding={outstanding}"
                    else:
                        concurrency += " (dnsperf default outstanding \u2248 100)"
                    bits.append(concurrency)
                if bits:
                    lines.append("**Recipe:** " + "; ".join(bits) + ".")
                    lines.append("")
            lines.append("</div>")
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def _write_scenarios_page(doc: dict[str, Any] | None) -> Path:
    """Inject catalog scenario intents into the hand-authored scenarios page."""
    path = OPERATOR_PERF / "scenarios.md"
    _ensure_page_shell(path, SCENARIOS_SHELL)
    body = _scenarios_body_markdown(doc)
    text = _inject_marked_section(
        path.read_text(encoding="utf-8"),
        start_marker=SCENARIOS_BODY_START,
        end_marker=SCENARIOS_BODY_END,
        body=body,
    )
    path.write_text(text, encoding="utf-8")
    return path


def generate_operator_docs_fragments(
    doc: dict[str, Any] | None = None,
    *,
    check_integrity: bool = True,
) -> list[Path]:
    """Render static SVG + CSV + markdown table fragments from committed reference JSON.

    When ``check_integrity`` is true (default), takeaway numeric claims on study
    pages must match generated evidence (Gate G5). Conflicts raise
    ``TakeawayIntegrityError``.
    """
    if doc is None:
        doc = load_latest_reference()
    GENERATED_DIR.mkdir(parents=True, exist_ok=True)
    OPERATOR_PERF.mkdir(parents=True, exist_ok=True)
    written: list[Path] = []
    stamp = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )

    meta_path = GENERATED_DIR / "README.txt"
    if doc is None:
        meta_path.write_text(
            "Generated performance fragments\n\n"
            "No promoted reference JSON is present yet. "
            "Charts and tables are omitted until a maintainer promotes a run.\n",
            encoding="utf-8",
        )
        written.append(meta_path)
        for pattern in ("*.svg", "*.csv", "chart-*.fragment.md", "study-*.fragment.md"):
            for stale in GENERATED_DIR.glob(pattern):
                stale.unlink()
        unavailable = GENERATED_DIR / "unavailable.fragment.md"
        unavailable.write_text(
            "_Reference measurements are not yet promoted for the curated spine._\n",
            encoding="utf-8",
        )
        written.append(unavailable)
        written.extend(write_annotation_include_fragments())
        written.extend(_inject_annotation_includes_into_pages())
        written.append(_write_reference_page(doc=None, stamp=stamp, fragments=[]))
        written.append(_write_scenarios_page(None))
        written.extend(
            _write_studies_docs(None, stamp=stamp, check_integrity=False)
        )
        return written

    profile = doc.get("lab_profile") or {}
    meta_path.write_text(
        "Generated performance fragments\n\n"
        f"Generated at {stamp} from promoted reference JSON "
        f"(lab profile {profile.get('id', '?')}). "
        "Do not edit by hand — regenerate with "
        "python3 -m perf.runner generate-docs or make perf-docs.\n",
        encoding="utf-8",
    )
    written.append(meta_path)

    # Drop stale chart artifacts before rewriting.
    for pattern in ("*.svg", "*.csv", "chart-*.fragment.md", "study-*.fragment.md"):
        for stale in GENERATED_DIR.glob(pattern):
            stale.unlink()

    charts = charts_for_document(doc, link_scenarios=True)
    chart_mds: list[Path] = []
    for chart in charts:
        paths = _write_chart_artifacts(chart)
        written.extend(paths)
        for p in paths:
            if p.name.startswith("chart-") and p.name.endswith(".fragment.md"):
                chart_mds.append(p)

    written.extend(write_annotation_include_fragments())
    written.extend(_inject_annotation_includes_into_pages())

    written.append(
        _write_reference_page(doc=doc, stamp=stamp, fragments=chart_mds)
    )
    written.append(_write_scenarios_page(doc))
    written.extend(
        _write_studies_docs(
            doc, stamp=stamp, check_integrity=check_integrity
        )
    )
    return written


REFERENCE_SHELL = """---
toc_depth: 3
toc_collapsible: true
---

# Performance reference results

Same-host comparisons from the named maintainer workstation lab profile
(`maintainer-ws-1`). Prefer reading each chart relative to its baseline cells
on that host. These are **not** service-level objectives. Reproduce on your
hardware with the
[harness instructions](/performance/reproduce.md) before making capacity decisions.
Absolute QPS is not a portable cross-host capacity claim.

""" + f"""{REFERENCE_BODY_START}
_Reference body is generated from committed JSON (run `make perf-docs`)._
{REFERENCE_BODY_END}
"""


def _reference_body_markdown(
    *,
    doc: dict[str, Any] | None,
    stamp: str,
    fragments: list[Path],
) -> str:
    lines = [
        f"_Generated {stamp} from committed reference JSON "
        "(no live load suite in docs CI)._",
        "",
        "## Lab profile",
        "",
    ]
    if doc is None:
        lines.extend(
            [
                "_No promoted reference JSON is committed yet._",
                "",
                "See [methodology](/performance/methodology.md) for how promotion works.",
                "",
            ]
        )
        return "\n".join(lines)

    profile = doc.get("lab_profile") or {}
    provenance = doc.get("provenance") or {}
    loadgen = provenance.get("loadgen") or {}
    lines.extend(
        [
            "| Field | Value |",
            "| --- | --- |",
            f"| Profile id | `{profile.get('id', '')}` |",
            f"| Display name | {profile.get('display_name', '')} |",
            f"| CPU | {profile.get('cpu_model', '')} |",
            f"| Cores (physical / logical) | "
            f"{profile.get('physical_cores', '')} / {profile.get('logical_cores', '')} |",
            f"| OS | {profile.get('os', '')} |",
            f"| Conduit | `{provenance.get('conduit_path', '')}` "
            f"({provenance.get('conduit_version', '')}) |",
            f"| Loadgen | {loadgen.get('tool', '')} "
            f"mode=`{loadgen.get('mode', '')}` |",
            f"| Run generated_at | `{doc.get('generated_at', '')}` |",
            "",
            "Underlying JSON: "
            "[`perf/results/references/`](https://github.com/egon1024/DNSConduit/tree/main/perf/results/references) "
            "(see `latest-reference.json` pointer in a checkout).",
            "",
            "Scenario intents: [Performance scenarios](/performance/scenarios.md). "
            "Decision-shaped comparisons: [Tuning evidence (studies)](/performance/studies/index.md).",
            "",
        ]
    )
    if not fragments:
        lines.append("## Charts and tables")
        lines.append("")
        lines.append("_No chart fragments were produced from this reference._\n")
    else:
        current_section: str | None = None
        for frag in fragments:
            section = _reference_chart_section(frag.name)
            if section != current_section:
                lines.append(f"## {section}")
                lines.append("")
                current_section = section
            text = frag.read_text(encoding="utf-8").strip()
            lines.append(text)
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def _write_reference_page(
    *,
    doc: dict[str, Any] | None,
    stamp: str,
    fragments: list[Path],
) -> Path:
    """Inject lab profile + charts into the hand-authored reference page."""
    path = OPERATOR_PERF / "reference.md"
    _ensure_page_shell(path, REFERENCE_SHELL)
    body = _reference_body_markdown(doc=doc, stamp=stamp, fragments=fragments)
    text = _inject_marked_section(
        path.read_text(encoding="utf-8"),
        start_marker=REFERENCE_BODY_START,
        end_marker=REFERENCE_BODY_END,
        body=body,
    )
    path.write_text(text, encoding="utf-8")
    return path


_ANN_TONE_ADMONITION = {
    "known_noise": "warning",
    "regressed_because": "warning",
    "improved_because": "tip",
    "context": "note",
}


def annotation_include_markdown(ann: Annotation) -> str:
    """Render one catalog annotation as an admonition fragment for page includes."""
    kind = _ANN_TONE_ADMONITION.get(ann.tone, "note")
    body_lines = ann.body.strip().splitlines() or [""]
    indented = "\n".join(f"    {line}" if line else "    " for line in body_lines)
    return f'!!! {kind} "{ann.title}"\n{indented}\n'


def write_annotation_include_fragments(
    anns: list[Annotation] | None = None,
) -> list[Path]:
    """Write ``includes/<id>.fragment.md`` for each catalog annotation (not nav pages)."""
    if anns is None:
        anns = load_annotations()
    includes = _includes_dir()
    includes.mkdir(parents=True, exist_ok=True)
    # Drop stale ann-*.fragment.md (keep hand-authored includes such as same-host-disclaimer).
    keep = {f"{a.id}.fragment.md" for a in anns}
    for stale in includes.glob("ann-*.fragment.md"):
        if stale.name not in keep:
            stale.unlink()
    # Legacy promote-index fragment (no longer assembled into reference.md).
    legacy = GENERATED_DIR / "annotations-from-reference.fragment.md"
    if legacy.is_file():
        legacy.unlink()
    # Dedicated catalog page removed — footnotes are includes only.
    ann_page = OPERATOR_PERF / "annotations.md"
    if ann_page.is_file():
        ann_page.unlink()
    written: list[Path] = []
    for ann in anns:
        path = includes / f"{ann.id}.fragment.md"
        path.write_text(annotation_include_markdown(ann), encoding="utf-8")
        written.append(path)
    return written


def _inject_annotation_includes_into_pages() -> list[Path]:
    """Fill ``<!-- perf-ann:<id>:start -->`` … ``:end -->`` markers from include fragments."""
    if not OPERATOR_PERF.is_dir():
        return []
    includes = _includes_dir()
    touched: list[Path] = []
    for path in sorted(OPERATOR_PERF.rglob("*.md")):
        if "generated" in path.parts:
            continue
        if path.name.endswith(".fragment.md"):
            continue
        text = path.read_text(encoding="utf-8")
        matches = list(_ANN_INCLUDE_START_RE.finditer(text))
        if not matches:
            continue
        new_text = text
        for match in reversed(matches):
            ann_id = match.group(1)
            start_marker = f"<!-- perf-ann:{ann_id}:start -->"
            end_marker = f"<!-- perf-ann:{ann_id}:end -->"
            frag_path = includes / f"{ann_id}.fragment.md"
            if frag_path.is_file():
                body = frag_path.read_text(encoding="utf-8")
            else:
                body = f"_Missing annotation include `{ann_id}`._\n"
            new_text = _inject_marked_section(
                new_text,
                start_marker=start_marker,
                end_marker=end_marker,
                body=body,
            )
        if new_text != text:
            path.write_text(new_text, encoding="utf-8")
            touched.append(path)
    return touched
