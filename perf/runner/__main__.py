"""CLI entry: python3 -m perf.runner …"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .catalog import (
    load_annotations,
    load_scenarios,
    load_studies,
    select_scenarios,
)
from .cpupower import check_host_cpu_power, require_cpu_power_ok
from .execute import DEFAULT_LOAD_SECONDS, build_run_document, run_scenario
from .udpbuffers import check_host_udp_buffers, require_udp_buffers_ok
from .loadgen import DEFAULT_IMAGE
from .paths import REFERENCES_DIR, ROOT, load_json
from .procs import find_stray_lab_processes, kill_stray_lab_processes
from .publish import (
    generate_operator_docs_fragments,
    merge_median_documents,
    promote_runs,
    publish_set_scenario_ids,
)
from .run_record import write_run_document
from ..render import FORMATS, render


def _suite_args(args: argparse.Namespace) -> list[str] | None:
    suites = getattr(args, "suite", None)
    if not suites:
        return None
    if isinstance(suites, str):
        return [suites]
    return list(suites)


def _scenario_args(args: argparse.Namespace) -> list[str] | None:
    ids = getattr(args, "scenario", None)
    if not ids:
        return None
    if isinstance(ids, str):
        return [ids]
    return list(ids)


def _study_args(args: argparse.Namespace) -> list[str] | None:
    ids = getattr(args, "study", None)
    if not ids:
        return None
    if isinstance(ids, str):
        return [ids]
    return list(ids)


def cmd_list(args: argparse.Namespace) -> int:
    if getattr(args, "studies", False):
        scenarios = load_scenarios()
        try:
            studies = load_studies(scenarios=scenarios)
        except ValueError as exc:
            print(str(exc), file=sys.stderr)
            return 1
        print("Studies:")
        for study in studies:
            pub = " published" if study.published else ""
            print(f"  {study.id}\tmembers={len(study.members)}{pub}")
            if args.verbose:
                print(f"    {study.question}")
                print(f"    members: {', '.join(study.members)}")
        return 0

    try:
        scenarios = select_scenarios(
            load_scenarios(),
            suites=_suite_args(args),
            scenario_ids=_scenario_args(args),
            curated_only=bool(getattr(args, "curated", False)),
            study_ids=_study_args(args),
            publish_set=bool(getattr(args, "publish_set", False)),
        )
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    print("Scenarios:")
    for sc in scenarios:
        curated = " curated" if sc.curated else ""
        print(f"  {sc.id}\tsuite={sc.suite}{curated}")
        if args.verbose and sc.intent:
            first = sc.intent.strip().splitlines()[0]
            print(f"    {first}")
    return 0


def cmd_annotations(args: argparse.Namespace) -> int:
    anns = load_annotations()
    print("Annotations:")
    if not anns:
        print("  (none)")
        return 0
    for ann in anns:
        print(f"  {ann.id}\ttone={ann.tone}\t{ann.title}")
        if args.verbose:
            for line in ann.body.strip().splitlines()[:4]:
                print(f"    {line}")
            if ann.related_releases:
                print(f"    releases: {', '.join(ann.related_releases)}")
    return 0


def cmd_render(args: argparse.Namespace) -> int:
    doc = load_json(Path(args.from_json))
    text = render(doc, args.format)
    if args.output:
        Path(args.output).write_text(text, encoding="utf-8")
        print(args.output)
    else:
        sys.stdout.write(text)
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    try:
        scenarios = select_scenarios(
            load_scenarios(),
            suites=_suite_args(args),
            scenario_ids=_scenario_args(args),
            curated_only=bool(args.curated),
            study_ids=_study_args(args),
            publish_set=bool(args.publish_set),
        )
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    if not scenarios:
        print("no scenarios matched filters", file=sys.stderr)
        return 1
    conduit = Path(args.conduit)
    if not conduit.is_file():
        print(f"conduit binary not found: {conduit}", file=sys.stderr)
        return 1
    if args.clients < 1:
        print("--clients must be >= 1", file=sys.stderr)
        return 2
    if args.dnsperf_threads < 1:
        print("--dnsperf-threads must be >= 1", file=sys.stderr)
        return 2
    if args.max_outstanding is not None and args.max_outstanding < 1:
        print("--max-outstanding must be >= 1 when set", file=sys.stderr)
        return 2

    power = check_host_cpu_power()
    allow_suboptimal = bool(getattr(args, "allow_suboptimal_cpu_power", False))
    power_err = require_cpu_power_ok(power, allow_suboptimal=allow_suboptimal)
    if power_err is not None:
        print(power_err, file=sys.stderr)
        return 2
    if power.status == "suboptimal" and allow_suboptimal:
        observed = ", ".join(sorted(power.governors)) or "(unknown)"
        print(
            f"warning: CPU governor(s) {observed} are suboptimal; "
            "continuing because --allow-suboptimal-cpu-power was set "
            "(results may be noisy)",
            file=sys.stderr,
        )

    udp_buf = check_host_udp_buffers()
    allow_udp = bool(getattr(args, "allow_suboptimal_udp_buffers", False))
    udp_err = require_udp_buffers_ok(udp_buf, allow_suboptimal=allow_udp)
    if udp_err is not None:
        print(udp_err, file=sys.stderr)
        return 2
    if udp_buf.status == "suboptimal" and allow_udp:
        print(
            f"warning: net.core.rmem_max={udp_buf.rmem_max} is below fixture "
            "listeners.rcvbuf; continuing because --allow-suboptimal-udp-buffers "
            "was set (expect Queries lost from kernel RcvbufErrors)",
            file=sys.stderr,
        )

    # Only refuse/kill processes recorded in a dead runner's PID ledger —
    # never a /proc cmdline guess (that matched Cursor agent shells before).
    strays = find_stray_lab_processes()
    if strays:
        if getattr(args, "kill_strays", False):
            kill_stray_lab_processes(strays)
            print(
                f"killed {len(strays)} ledger-tracked orphan(s) before measuring",
                file=sys.stderr,
            )
        else:
            print(
                "refusing to measure: ledger-tracked orphans from a dead runner "
                "are still alive. They keep their affinity, so they tax every "
                "cell measured after them.",
                file=sys.stderr,
            )
            for stray in strays:
                print(
                    f"  pid {stray.pid} ({stray.kind}): {stray.cmdline[:120]}",
                    file=sys.stderr,
                )
            print("Re-run with --kill-strays to clear them.", file=sys.stderr)
            return 2

    run_annotation_ids = list(args.annotation_id or [])
    # scenario_id -> [annotation ids]
    per_scenario: dict[str, list[str]] = {}
    for item in args.scenario_annotation or []:
        if "=" not in item:
            print(
                f"invalid --scenario-annotation {item!r}; expected scenario_id=annotation_id",
                file=sys.stderr,
            )
            return 2
        sid, aid = item.split("=", 1)
        sid, aid = sid.strip(), aid.strip()
        if not sid or not aid:
            print(f"invalid --scenario-annotation {item!r}", file=sys.stderr)
            return 2
        per_scenario.setdefault(sid, []).append(aid)

    catalog_ids = {a.id for a in load_annotations()}
    for aid in run_annotation_ids:
        if aid not in catalog_ids:
            print(f"warning: annotation id not in catalog: {aid}", file=sys.stderr)
    for aids in per_scenario.values():
        for aid in aids:
            if aid not in catalog_ids:
                print(f"warning: annotation id not in catalog: {aid}", file=sys.stderr)

    otlp = Path(args.otlp_tracer) if args.otlp_tracer else None
    dnstap = Path(args.dnstap_tracer) if args.dnstap_tracer else None
    ctl = Path(args.conduitctl) if args.conduitctl else None
    results = []
    for sc in scenarios:
        print(f"running {sc.id} …", file=sys.stderr)
        sc_anns = list(per_scenario.get(sc.id) or [])
        # When a single scenario is selected, run-level ids also attach to it.
        if len(scenarios) == 1 and run_annotation_ids:
            for aid in run_annotation_ids:
                if aid not in sc_anns:
                    sc_anns.append(aid)
        results.append(
            run_scenario(
                sc,
                conduit=conduit,
                loadgen_mode=args.loadgen_mode,
                loadgen_image=args.loadgen_image,
                time_s=args.time,
                warmup_s=args.warmup,
                clients=args.clients,
                dnsperf_threads=args.dnsperf_threads,
                max_outstanding=args.max_outstanding,
                otlp_tracer=otlp,
                dnstap_tracer=dnstap,
                conduitctl=ctl,
                zdu=args.zdu,
                annotation_ids=sc_anns or None,
            )
        )

    doc = build_run_document(
        results,
        conduit=conduit,
        profile_id=args.profile_id,
        loadgen_mode=args.loadgen_mode,
        loadgen_image=args.loadgen_image,
        warmup_s=args.warmup,
        time_s=args.time,
        clients=args.clients,
        dnsperf_threads=args.dnsperf_threads,
        max_outstanding=args.max_outstanding,
        run_annotation_ids=run_annotation_ids or None,
    )
    out = write_run_document(doc, path=Path(args.output) if args.output else None)
    print(out)
    if args.render:
        sys.stdout.write(render(doc, args.render))
    return 0


def cmd_promote(args: argparse.Namespace) -> int:
    sources = [Path(p) for p in args.from_json]
    for p in sources:
        if not p.is_file():
            print(f"run JSON not found: {p}", file=sys.stderr)
            return 1
    dest = promote_runs(
        sources,
        name=args.name,
        annotation_ids=list(args.annotation_id or []) or None,
        profile_id=args.profile_id,
        thin_spine=bool(args.thin_spine),
        publish_set=bool(args.publish_set),
    )
    print(dest)
    print(REFERENCES_DIR / "latest-reference.json")
    return 0


def cmd_merge_median(args: argparse.Namespace) -> int:
    sources = [Path(p) for p in args.from_json]
    for p in sources:
        if not p.is_file():
            print(f"run JSON not found: {p}", file=sys.stderr)
            return 1
    docs = [load_json(p) for p in sources]
    merged = merge_median_documents(docs)
    out = write_run_document(merged, path=Path(args.output) if args.output else None)
    print(out)
    return 0


def cmd_generate_docs(args: argparse.Namespace) -> int:
    from perf.runner.integrity import TakeawayIntegrityError

    doc = None
    if args.from_json:
        doc = load_json(Path(args.from_json))
    try:
        written = generate_operator_docs_fragments(doc)
    except TakeawayIntegrityError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    for path in written:
        try:
            print(path.relative_to(ROOT))
        except ValueError:
            print(path)
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="python3 -m perf.runner")
    sub = p.add_subparsers(dest="command", required=True)

    list_p = sub.add_parser("list", help="List catalog scenarios")
    list_p.add_argument(
        "--suite",
        action="append",
        help="Filter by suite (repeatable)",
    )
    list_p.add_argument(
        "--scenario",
        action="append",
        help="Filter by scenario id (repeatable)",
    )
    list_p.add_argument(
        "--study",
        action="append",
        help="Filter by study id (expand to member scenarios; repeatable)",
    )
    list_p.add_argument(
        "--publish-set",
        action="store_true",
        help="Union of members from studies marked published",
    )
    list_p.add_argument(
        "--studies",
        action="store_true",
        help="List study catalog instead of scenarios",
    )
    list_p.add_argument(
        "--curated",
        action="store_true",
        help="Only scenarios marked curated in the catalog",
    )
    list_p.add_argument("-v", "--verbose", action="store_true")
    list_p.set_defaults(func=cmd_list)

    ann_p = sub.add_parser("annotations", help="List annotation catalog ids")
    ann_p.add_argument("-v", "--verbose", action="store_true")
    ann_p.set_defaults(func=cmd_annotations)

    render_p = sub.add_parser("render", help="Render an existing run JSON")
    render_p.add_argument("--from", dest="from_json", required=True, help="Run JSON path")
    render_p.add_argument(
        "--format",
        choices=sorted(FORMATS),
        default="plain",
    )
    render_p.add_argument("-o", "--output", help="Write to file instead of stdout")
    render_p.set_defaults(func=cmd_render)

    run_p = sub.add_parser("run", help="Run scenarios against a Conduit binary")
    run_p.add_argument(
        "--conduit",
        required=True,
        help="Path to Conduit binary (no cargo invoked for SUT)",
    )
    run_p.add_argument(
        "--suite",
        action="append",
        help="Filter by suite (repeatable)",
    )
    run_p.add_argument(
        "--scenario",
        action="append",
        help="Filter by scenario id (repeatable)",
    )
    run_p.add_argument(
        "--study",
        action="append",
        help="Run member scenarios of a study (repeatable)",
    )
    run_p.add_argument(
        "--publish-set",
        action="store_true",
        help="Run union of members from studies marked published",
    )
    run_p.add_argument(
        "--curated",
        action="store_true",
        help="Only scenarios marked curated in the catalog",
    )
    run_p.add_argument("--profile-id", default="local", help="Lab profile id for this run")
    run_p.add_argument(
        "--loadgen-mode",
        choices=["docker", "native"],
        default="docker",
        help="dnsperf invocation (default: docker)",
    )
    run_p.add_argument("--loadgen-image", default=DEFAULT_IMAGE)
    run_p.add_argument(
        "--time",
        type=int,
        default=None,
        help=(
            "dnsperf -l seconds (overrides a scenario's duration_s; "
            f"default {DEFAULT_LOAD_SECONDS})"
        ),
    )
    run_p.add_argument("--warmup", type=float, default=2.0)
    run_p.add_argument(
        "--clients",
        type=int,
        default=4,
        metavar="N",
        help="dnsperf -c (parallel clients; default: 4)",
    )
    run_p.add_argument(
        "--dnsperf-threads",
        type=int,
        default=2,
        metavar="N",
        help="dnsperf -T (worker threads; default: 2)",
    )
    run_p.add_argument(
        "--max-outstanding",
        type=int,
        default=None,
        metavar="N",
        help=(
            "dnsperf -q (max outstanding queries). "
            "Omit to use dnsperf default (100). Published cells omit this."
        ),
    )
    run_p.add_argument("--otlp-tracer", help="Path to conduit-otlp-metrics-tracer")
    run_p.add_argument(
        "--dnstap-tracer",
        help="Path to conduit-dnstap-tracer (default: sibling of --conduit)",
    )
    run_p.add_argument(
        "--conduitctl",
        help="Path to conduitctl (default: sibling of --conduit)",
    )
    run_p.add_argument(
        "--zdu",
        action="store_true",
        help="Binary under test supports zero-downtime upgrade",
    )
    run_p.add_argument(
        "--allow-suboptimal-cpu-power",
        action="store_true",
        help=(
            "Allow run when CPU frequency governors are not all 'performance' "
            "(default: refuse — powersave/schedutil/mixed governors add host noise)"
        ),
    )
    run_p.add_argument(
        "--allow-suboptimal-udp-buffers",
        action="store_true",
        help=(
            "Allow run when net.core.rmem_max is below fixture listeners.rcvbuf "
            "(default: refuse — undersized UDP recv buffers cause Queries lost "
            "via kernel RcvbufErrors)"
        ),
    )
    run_p.add_argument(
        "--kill-strays",
        action="store_true",
        help=(
            "SIGKILL only ledger-tracked orphans from a dead runner (never a "
            "/proc cmdline guess) instead of refusing to measure alongside them"
        ),
    )
    run_p.add_argument(
        "--annotation-id",
        action="append",
        default=[],
        help="Attach run-level annotation id (repeatable; also attaches to a single selected scenario)",
    )
    run_p.add_argument(
        "--scenario-annotation",
        action="append",
        default=[],
        help="Attach annotation to a scenario: scenario_id=annotation_id (repeatable)",
    )
    run_p.add_argument("-o", "--output", help="Run JSON output path")
    run_p.add_argument(
        "--render",
        choices=sorted(FORMATS),
        help="Also print a rendered format after writing JSON",
    )
    run_p.set_defaults(func=cmd_run)

    promote_p = sub.add_parser(
        "promote",
        help="Promote run JSON into results/references/ (manual PR path)",
    )
    promote_p.add_argument(
        "--from",
        dest="from_json",
        action="append",
        required=True,
        help="Source run JSON (repeatable; scenarios are merged)",
    )
    promote_p.add_argument(
        "--name",
        default="thin-spine",
        help="Reference basename under results/references/ (default: thin-spine)",
    )
    promote_p.add_argument(
        "--profile-id",
        default="maintainer-ws-1",
        help="Blessed lab profile id to record on the promoted document",
    )
    promote_p.add_argument(
        "--annotation-id",
        action="append",
        default=[],
        help="Ensure these run-level annotation ids are present (repeatable)",
    )
    promote_p.add_argument(
        "--thin-spine",
        action="store_true",
        help="Keep only the legacy thin curated spine scenario ids",
    )
    promote_p.add_argument(
        "--publish-set",
        action="store_true",
        help="Keep union of members from studies marked published (preferred curated path)",
    )
    promote_p.set_defaults(func=cmd_promote)

    median_p = sub.add_parser(
        "merge-median",
        help=(
            "Combine N same-shape round run JSONs into one via per-scenario "
            "field median (reduces single-shot host noise before promote)"
        ),
    )
    median_p.add_argument(
        "--from",
        dest="from_json",
        action="append",
        required=True,
        help="Round run JSON (repeatable; at least 2 required)",
    )
    median_p.add_argument("-o", "--output", help="Run JSON output path")
    median_p.set_defaults(func=cmd_merge_median)

    gen_p = sub.add_parser(
        "generate-docs",
        help="Generate operator-docs fragments from committed reference JSON (no live bench)",
    )
    gen_p.add_argument(
        "--from",
        dest="from_json",
        help="Optional explicit reference JSON (default: latest-reference pointer)",
    )
    gen_p.set_defaults(func=cmd_generate_docs)

    return p


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
