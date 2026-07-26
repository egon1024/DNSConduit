"""CLI entry: python3 -m perf.runner …"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .catalog import filter_scenarios, load_annotations, load_scenarios
from .execute import build_run_document, run_scenario
from .loadgen import DEFAULT_IMAGE
from .paths import load_json
from .run_record import write_run_document
from ..render import FORMATS, render


def cmd_list(args: argparse.Namespace) -> int:
    scenarios = filter_scenarios(
        load_scenarios(), suite=args.suite, scenario_id=args.scenario
    )
    print("Scenarios:")
    for sc in scenarios:
        curated = " curated" if sc.curated else ""
        print(f"  {sc.id}\tsuite={sc.suite}{curated}")
        if args.verbose and sc.intent:
            first = sc.intent.strip().splitlines()[0]
            print(f"    {first}")
    return 0


def cmd_annotations(_args: argparse.Namespace) -> int:
    anns = load_annotations()
    print("Annotations:")
    if not anns:
        print("  (none)")
        return 0
    for ann in anns:
        print(f"  {ann.id}\ttone={ann.tone}\t{ann.title}")
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
    scenarios = filter_scenarios(
        load_scenarios(), suite=args.suite, scenario_id=args.scenario
    )
    if not scenarios:
        print("no scenarios matched filters", file=sys.stderr)
        return 1
    conduit = Path(args.conduit)
    if not conduit.is_file():
        print(f"conduit binary not found: {conduit}", file=sys.stderr)
        return 1

    otlp = Path(args.otlp_tracer) if args.otlp_tracer else None
    results = []
    for sc in scenarios:
        print(f"running {sc.id} …", file=sys.stderr)
        results.append(
            run_scenario(
                sc,
                conduit=conduit,
                loadgen_mode=args.loadgen_mode,
                loadgen_image=args.loadgen_image,
                time_s=args.time,
                warmup_s=args.warmup,
                otlp_tracer=otlp,
                zdu=args.zdu,
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
    )
    out = write_run_document(doc, path=Path(args.output) if args.output else None)
    print(out)
    if args.render:
        sys.stdout.write(render(doc, args.render))
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="python3 -m perf.runner")
    sub = p.add_subparsers(dest="command", required=True)

    list_p = sub.add_parser("list", help="List catalog scenarios")
    list_p.add_argument("--suite", help="Filter by suite")
    list_p.add_argument("--scenario", help="Filter by scenario id")
    list_p.add_argument("-v", "--verbose", action="store_true")
    list_p.set_defaults(func=cmd_list)

    ann_p = sub.add_parser("annotations", help="List annotation catalog ids")
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
    run_p.add_argument("--suite", help="Filter by suite")
    run_p.add_argument("--scenario", help="Filter by scenario id")
    run_p.add_argument("--profile-id", default="local", help="Lab profile id for this run")
    run_p.add_argument(
        "--loadgen-mode",
        choices=["docker", "native"],
        default="docker",
        help="dnsperf invocation (default: docker)",
    )
    run_p.add_argument("--loadgen-image", default=DEFAULT_IMAGE)
    run_p.add_argument("--time", type=int, default=10, help="dnsperf -l seconds")
    run_p.add_argument("--warmup", type=float, default=2.0)
    run_p.add_argument("--otlp-tracer", help="Path to conduit-otlp-metrics-tracer")
    run_p.add_argument(
        "--zdu",
        action="store_true",
        help="Binary under test supports zero-downtime upgrade",
    )
    run_p.add_argument("-o", "--output", help="Run JSON output path")
    run_p.add_argument(
        "--render",
        choices=sorted(FORMATS),
        help="Also print a rendered format after writing JSON",
    )
    run_p.set_defaults(func=cmd_run)

    return p


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
