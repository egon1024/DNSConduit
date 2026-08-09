"""Callable facade for the performance harness (CLI + TUI share this)."""

from __future__ import annotations

import hashlib
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Literal

from .catalog import (
    load_annotations,
    load_scenarios,
    load_studies,
    select_scenarios,
)
from .cpupower import check_host_cpu_power, require_cpu_power_ok
from .execute import build_run_document, run_scenario
from .integrity import TakeawayIntegrityError, verify_studies_integrity
from .loadgen import DEFAULT_IMAGE
from .paths import REFERENCES_DIR, ROOT, load_json, write_json
from .lab_ports import refuse_if_lab_ports_busy
from .procs import find_stray_lab_processes, kill_stray_lab_processes
from .publish import (
    GENERATED_DIR,
    LATEST_POINTER,
    OPERATOR_PERF,
    generate_operator_docs_fragments,
    load_latest_reference,
    merge_median_documents,
    promote_runs,
)
from .run_record import write_run_document
from .udpbuffers import check_host_udp_buffers, require_udp_buffers_ok
from ..render import FORMATS, render
from ..render.charts import charts_for_studies

SOURCE_REFERENCE_STAMP = GENERATED_DIR / "source-reference.stamp"


def file_fingerprint(path: Path) -> str | None:
    """Return a stable content fingerprint, or None if the path is missing."""
    if not path.is_file():
        return None
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def write_source_reference_stamp(*, source_path: Path, fingerprint: str) -> Path:
    """Record which reference JSON produced the current generated docs."""
    GENERATED_DIR.mkdir(parents=True, exist_ok=True)
    payload = {
        "path": str(source_path),
        "fingerprint": fingerprint,
    }
    write_json(SOURCE_REFERENCE_STAMP, payload)
    return SOURCE_REFERENCE_STAMP


def read_source_reference_stamp() -> dict[str, Any] | None:
    if not SOURCE_REFERENCE_STAMP.is_file():
        return None
    return load_json(SOURCE_REFERENCE_STAMP)


def resolve_latest_reference_path() -> Path | None:
    if not LATEST_POINTER.is_file():
        return None
    pointer = load_json(LATEST_POINTER)
    ref_name = pointer.get("reference")
    if ref_name:
        path = REFERENCES_DIR / ref_name
        if path.is_file():
            return path
    rel = pointer.get("path")
    if rel:
        path = ROOT / rel
        if path.is_file():
            return path
    return None


@dataclass(frozen=True)
class RunProgressEvent:
    kind: Literal[
        "cycle_start",
        "cycle_done",
        "scenario_start",
        "scenario_done",
        "message",
        "cancelled",
    ]
    scenario_id: str | None = None
    index: int = 0
    total: int = 0
    cycle: int = 1
    cycles: int = 1
    message: str = ""


ProgressCallback = Callable[[RunProgressEvent], None]


@dataclass
class RunParams:
    conduit: Path
    suites: list[str] | None = None
    scenario_ids: list[str] | None = None
    study_ids: list[str] | None = None
    curated_only: bool = False
    publish_set: bool = False
    profile_id: str = "local"
    loadgen_mode: str = "docker"
    loadgen_image: str = DEFAULT_IMAGE
    time_s: int | None = None
    warmup_s: float = 2.0
    clients: int = 4
    dnsperf_threads: int = 2
    max_outstanding: int | None = None
    otlp_tracer: Path | None = None
    dnstap_tracer: Path | None = None
    conduitctl: Path | None = None
    zdu: bool = False
    allow_suboptimal_cpu_power: bool = False
    allow_suboptimal_udp_buffers: bool = False
    kill_strays: bool = False
    annotation_ids: list[str] = field(default_factory=list)
    scenario_annotations: dict[str, list[str]] = field(default_factory=dict)
    output: Path | None = None
    cycles: int = 1
    cancel_event: threading.Event | None = None
    on_progress: ProgressCallback | None = None


class PreflightError(Exception):
    """Host/lab preflight refused the run (maps to CLI exit 2)."""

    def __init__(self, message: str, *, exit_code: int = 2) -> None:
        super().__init__(message)
        self.exit_code = exit_code


class FacadeError(Exception):
    """User/input error (maps to CLI exit 1)."""

    def __init__(self, message: str, *, exit_code: int = 1) -> None:
        super().__init__(message)
        self.exit_code = exit_code


def list_scenario_summaries(
    *,
    suites: list[str] | None = None,
    scenario_ids: list[str] | None = None,
    study_ids: list[str] | None = None,
    curated_only: bool = False,
    publish_set: bool = False,
) -> list[tuple[str, str, bool, str]]:
    """Return (id, suite, curated, summary) rows."""
    try:
        scenarios = select_scenarios(
            load_scenarios(),
            suites=suites,
            scenario_ids=scenario_ids,
            curated_only=curated_only,
            study_ids=study_ids,
            publish_set=publish_set,
        )
    except ValueError as exc:
        raise FacadeError(str(exc)) from exc
    return [(sc.id, sc.suite, sc.curated, sc.summary or "") for sc in scenarios]


def list_study_summaries() -> list[tuple[str, int, bool, str]]:
    """Return (id, member_count, published, question)."""
    scenarios = load_scenarios()
    try:
        studies = load_studies(scenarios=scenarios)
    except ValueError as exc:
        raise FacadeError(str(exc)) from exc
    return [(s.id, len(s.members), s.published, s.question) for s in studies]


def render_run(from_json: Path, fmt: str, output: Path | None = None) -> str:
    if fmt not in FORMATS:
        raise FacadeError(f"unknown render format: {fmt}")
    doc = load_json(from_json)
    text = render(doc, fmt)
    if output is not None:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(text, encoding="utf-8")
    return text


def merge_median(sources: list[Path], output: Path | None = None) -> Path:
    if len(sources) < 2:
        raise FacadeError("merge-median requires at least 2 source run JSON files")
    for p in sources:
        if not p.is_file():
            raise FacadeError(f"run JSON not found: {p}")
    docs = [load_json(p) for p in sources]
    merged = merge_median_documents(docs)
    return write_run_document(merged, path=output)


def promote(
    sources: list[Path],
    *,
    name: str = "thin-spine",
    profile_id: str = "maintainer-ws-1",
    annotation_ids: list[str] | None = None,
    thin_spine: bool = False,
    publish_set: bool = False,
) -> Path:
    for p in sources:
        if not p.is_file():
            raise FacadeError(f"run JSON not found: {p}")
    return promote_runs(
        sources,
        name=name,
        annotation_ids=annotation_ids,
        profile_id=profile_id,
        thin_spine=thin_spine,
        publish_set=publish_set,
    )


def generate_docs(
    from_json: Path | None = None,
    *,
    check_integrity: bool = True,
    write_stamp: bool = True,
) -> list[Path]:
    source_path: Path | None = None
    doc = None
    if from_json is not None:
        source_path = Path(from_json)
        doc = load_json(source_path)
    else:
        source_path = resolve_latest_reference_path()
        doc = load_latest_reference()
    written = generate_operator_docs_fragments(doc, check_integrity=check_integrity)
    if write_stamp and source_path is not None and source_path.is_file():
        fp = file_fingerprint(source_path)
        if fp is not None:
            write_source_reference_stamp(source_path=source_path, fingerprint=fp)
            written.append(SOURCE_REFERENCE_STAMP)
    return written


def check_takeaway_integrity(from_json: Path | None = None) -> None:
    """Verify study takeaways against charts for a reference doc (no writes)."""
    if from_json is not None:
        doc = load_json(Path(from_json))
    else:
        doc = load_latest_reference()
    if doc is None:
        raise FacadeError("no promoted reference JSON available for integrity check")
    scenarios = load_scenarios()
    try:
        studies = load_studies(scenarios=scenarios)
    except ValueError as exc:
        raise FacadeError(str(exc)) from exc
    studies_with_charts = charts_for_studies(doc, studies, published_only=True)
    studies_docs = OPERATOR_PERF / "studies"
    page_text_by_id: dict[str, str] = {}
    for study, _charts in studies_with_charts:
        path = studies_docs / f"{study.id}.md"
        if path.is_file():
            page_text_by_id[study.id] = path.read_text(encoding="utf-8")
    errors = verify_studies_integrity(
        studies_with_charts, page_text_by_id=page_text_by_id
    )
    if errors:
        raise TakeawayIntegrityError(errors)


def _emit(params: RunParams, event: RunProgressEvent) -> None:
    if params.on_progress is not None:
        params.on_progress(event)


def _preflight(params: RunParams) -> list[str]:
    """Return warning lines; raise PreflightError on hard refuse."""
    warnings: list[str] = []
    if not params.conduit.is_file():
        raise FacadeError(f"conduit binary not found: {params.conduit}")
    if params.clients < 1:
        raise PreflightError("--clients must be >= 1")
    if params.dnsperf_threads < 1:
        raise PreflightError("--dnsperf-threads must be >= 1")
    if params.max_outstanding is not None and params.max_outstanding < 1:
        raise PreflightError("--max-outstanding must be >= 1 when set")
    if params.cycles < 1:
        raise PreflightError("cycles must be >= 1")

    power = check_host_cpu_power()
    power_err = require_cpu_power_ok(
        power, allow_suboptimal=params.allow_suboptimal_cpu_power
    )
    if power_err is not None:
        raise PreflightError(power_err)
    if power.status == "suboptimal" and params.allow_suboptimal_cpu_power:
        observed = ", ".join(sorted(power.governors)) or "(unknown)"
        warnings.append(
            f"warning: CPU governor(s) {observed} are suboptimal; "
            "continuing because allow_suboptimal_cpu_power was set "
            "(results may be noisy)"
        )

    udp_buf = check_host_udp_buffers()
    udp_err = require_udp_buffers_ok(
        udp_buf, allow_suboptimal=params.allow_suboptimal_udp_buffers
    )
    if udp_err is not None:
        raise PreflightError(udp_err)
    if udp_buf.status == "suboptimal" and params.allow_suboptimal_udp_buffers:
        warnings.append(
            f"warning: net.core.rmem_max={udp_buf.rmem_max} is below fixture "
            "listeners.rcvbuf; continuing because allow_suboptimal_udp_buffers "
            "was set (expect Queries lost from kernel RcvbufErrors)"
        )

    strays = find_stray_lab_processes()
    if strays:
        if params.kill_strays:
            kill_stray_lab_processes(strays)
            warnings.append(
                f"killed {len(strays)} ledger-tracked orphan(s) before measuring"
            )
        else:
            lines = [
                "refusing to measure: ledger-tracked orphans from a dead runner "
                "are still alive. They keep their affinity, so they tax every "
                "cell measured after them.",
            ]
            for stray in strays:
                lines.append(
                    f"  pid {stray.pid} ({stray.kind}): {stray.cmdline[:120]}"
                )
            lines.append("Re-run with kill_strays to clear them.")
            raise PreflightError("\n".join(lines))

    port_err = refuse_if_lab_ports_busy()
    if port_err is not None:
        raise PreflightError(port_err)
    return warnings


def run_benchmarks(params: RunParams) -> list[Path]:
    """Run selected scenarios for ``params.cycles`` rounds; return written paths."""
    try:
        scenarios = select_scenarios(
            load_scenarios(),
            suites=params.suites,
            scenario_ids=params.scenario_ids,
            curated_only=params.curated_only,
            study_ids=params.study_ids,
            publish_set=params.publish_set,
        )
    except ValueError as exc:
        raise FacadeError(str(exc)) from exc
    if not scenarios:
        raise FacadeError("no scenarios matched filters")

    warnings = _preflight(params)
    for w in warnings:
        _emit(
            params,
            RunProgressEvent(kind="message", message=w, cycles=params.cycles),
        )

    catalog_ids = {a.id for a in load_annotations()}
    for aid in params.annotation_ids:
        if aid not in catalog_ids:
            _emit(
                params,
                RunProgressEvent(
                    kind="message",
                    message=f"warning: annotation id not in catalog: {aid}",
                    cycles=params.cycles,
                ),
            )
    for aids in params.scenario_annotations.values():
        for aid in aids:
            if aid not in catalog_ids:
                _emit(
                    params,
                    RunProgressEvent(
                        kind="message",
                        message=f"warning: annotation id not in catalog: {aid}",
                        cycles=params.cycles,
                    ),
                )

    written: list[Path] = []
    for cycle in range(1, params.cycles + 1):
        if params.cancel_event is not None and params.cancel_event.is_set():
            _emit(
                params,
                RunProgressEvent(
                    kind="cancelled",
                    cycle=cycle,
                    cycles=params.cycles,
                    message="cancelled before cycle start",
                ),
            )
            break
        _emit(
            params,
            RunProgressEvent(
                kind="cycle_start",
                cycle=cycle,
                cycles=params.cycles,
                message=f"cycle {cycle}/{params.cycles}",
            ),
        )
        results = []
        total = len(scenarios)
        cancelled = False
        for index, sc in enumerate(scenarios, start=1):
            if params.cancel_event is not None and params.cancel_event.is_set():
                _emit(
                    params,
                    RunProgressEvent(
                        kind="cancelled",
                        cycle=cycle,
                        cycles=params.cycles,
                        index=index,
                        total=total,
                        message="cancelled between scenarios",
                    ),
                )
                cancelled = True
                break
            _emit(
                params,
                RunProgressEvent(
                    kind="scenario_start",
                    scenario_id=sc.id,
                    index=index,
                    total=total,
                    cycle=cycle,
                    cycles=params.cycles,
                    message=f"running {sc.id} …",
                ),
            )
            sc_anns = list(params.scenario_annotations.get(sc.id) or [])
            if len(scenarios) == 1 and params.annotation_ids:
                for aid in params.annotation_ids:
                    if aid not in sc_anns:
                        sc_anns.append(aid)
            results.append(
                run_scenario(
                    sc,
                    conduit=params.conduit,
                    loadgen_mode=params.loadgen_mode,
                    loadgen_image=params.loadgen_image,
                    time_s=params.time_s,
                    warmup_s=params.warmup_s,
                    clients=params.clients,
                    dnsperf_threads=params.dnsperf_threads,
                    max_outstanding=params.max_outstanding,
                    otlp_tracer=params.otlp_tracer,
                    dnstap_tracer=params.dnstap_tracer,
                    conduitctl=params.conduitctl,
                    zdu=params.zdu,
                    annotation_ids=sc_anns or None,
                )
            )
            _emit(
                params,
                RunProgressEvent(
                    kind="scenario_done",
                    scenario_id=sc.id,
                    index=index,
                    total=total,
                    cycle=cycle,
                    cycles=params.cycles,
                ),
            )
        if results:
            doc = build_run_document(
                results,
                conduit=params.conduit,
                profile_id=params.profile_id,
                loadgen_mode=params.loadgen_mode,
                loadgen_image=params.loadgen_image,
                warmup_s=params.warmup_s,
                time_s=params.time_s,
                clients=params.clients,
                dnsperf_threads=params.dnsperf_threads,
                max_outstanding=params.max_outstanding,
                run_annotation_ids=params.annotation_ids or None,
            )
            out_path = _cycle_output_path(params, cycle)
            path = write_run_document(doc, path=out_path)
            written.append(path)
            _emit(
                params,
                RunProgressEvent(
                    kind="cycle_done",
                    cycle=cycle,
                    cycles=params.cycles,
                    message=str(path),
                ),
            )
        if cancelled:
            break
    return written


def _cycle_output_path(params: RunParams, cycle: int) -> Path | None:
    if params.output is None:
        return None
    if params.cycles == 1:
        return params.output
    out = params.output
    if out.suffix == ".json" and params.cycles > 1:
        return out.parent / f"{out.stem}-r{cycle}{out.suffix}"
    out.mkdir(parents=True, exist_ok=True)
    return out / f"r{cycle}.json"
