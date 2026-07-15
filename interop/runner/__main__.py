"""CLI entry: python3 -m interop.runner …"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from typing import Iterable

from . import __version__
from .cases import Case, load_cases
from .catalog import Peer, load_peers
from .compose import CellStack, dig_query, docker_available, image_digest
from .fingerprint import compute_inputs_fingerprint, fingerprint_report, git_head
from .generate_matrix import generate_matrix
from .oracles import evaluate_oracles
from .paths import RESULTS_FILE, write_json
from .setup_ir import parse_peer_setup


def _filter_peers(peers: list[Peer], peer: str | None, version: str | None) -> list[Peer]:
    out = peers
    if peer:
        out = [p for p in out if p.id == peer or p.product.casefold() == peer.casefold()]
    if version:
        out = [p for p in out if p.version == version]
    return out


def _filter_cases(
    cases: list[Case],
    *,
    case: str | None,
    suite: str | None,
) -> list[Case]:
    out = cases
    if case:
        out = [c for c in out if c.id == case]
    if suite:
        out = [c for c in out if suite in c.suites]
    return out


def cmd_fingerprint(_args: argparse.Namespace) -> int:
    report = fingerprint_report()
    print(report["inputs_fingerprint"])
    if _args.verbose:
        print(json.dumps(report, indent=2), file=sys.stderr)
    return 0


def cmd_generate_matrix(_args: argparse.Namespace) -> int:
    path = generate_matrix()
    print(path)
    return 0


def cmd_list(_args: argparse.Namespace) -> int:
    print("Peers (publisher A–Z):")
    for peer in load_peers():
        print(f"  {peer.id}\t{peer.publisher}\t{peer.product}\t{peer.version}\t{peer.role}")
    print("Cases:")
    for case in load_cases():
        print(f"  {case.id}\tsuites={','.join(case.suites)}")
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    peers = _filter_peers(load_peers(), args.peer, args.version)
    cases = _filter_cases(load_cases(), case=args.case, suite=args.suite)
    profiles = [args.profile] if args.profile else ["forward-only"]
    conduit_image = args.conduit_image
    dry_run = args.dry_run or not docker_available()

    if not cases:
        print("no cases matched filters", file=sys.stderr)
        return 1
    if not peers:
        print("no peers matched filters", file=sys.stderr)
        return 1

    cells: list[dict] = []
    existing: dict[tuple, dict] = {}
    if args.merge and RESULTS_FILE.is_file():
        from .paths import load_json

        prev = load_json(RESULTS_FILE)
        for cell in prev.get("cells", []):
            existing[(cell["case_id"], cell["peer_id"], cell["profile_id"])] = cell

    for profile_id in profiles:
        for peer in peers:
            for case in cases:
                key = (case.id, peer.id, profile_id)
                if not case.applies_to(role=peer.role, profile_id=profile_id, peer_id=peer.id):
                    cell = {
                        "case_id": case.id,
                        "peer_id": peer.id,
                        "profile_id": profile_id,
                        "outcome": "skip",
                        "detail": "applicability",
                        "oracles": [o.get("kind") for o in case.oracles],
                    }
                    cells.append(cell)
                    print(f"SKIP {case.id} @ {peer.id} ({profile_id})")
                    continue

                if dry_run:
                    cell = {
                        "case_id": case.id,
                        "peer_id": peer.id,
                        "profile_id": profile_id,
                        "outcome": "skip",
                        "detail": "dry-run (no docker execution)",
                        "oracles": [o.get("kind") for o in case.oracles],
                    }
                    cells.append(cell)
                    print(f"DRY  {case.id} @ {peer.id} ({profile_id})")
                    continue

                try:
                    outcome, detail = _run_cell(
                        case=case,
                        peer=peer,
                        profile_id=profile_id,
                        conduit_image=conduit_image,
                        host_port=args.host_port,
                        qname=args.qname,
                    )
                except Exception as exc:  # noqa: BLE001 — cell infra must not abort the suite
                    outcome, detail = "fail", f"cell setup/run error: {exc}"
                cell = {
                    "case_id": case.id,
                    "peer_id": peer.id,
                    "profile_id": profile_id,
                    "outcome": outcome,
                    "detail": detail,
                    "oracles": [o.get("kind") for o in case.oracles],
                }
                cells.append(cell)
                print(f"{outcome.upper():5} {case.id} @ {peer.id} ({profile_id}) — {detail}")

    if args.merge:
        for cell in cells:
            existing[(cell["case_id"], cell["peer_id"], cell["profile_id"])] = cell
        cells = list(existing.values())

    digest = image_digest(conduit_image) if not dry_run else "dry-run"
    payload = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "provenance": {
            "conduit_image": conduit_image,
            "conduit_image_digest": digest,
            "conduit_version": args.conduit_version,
            "git_head": git_head(),
            "runner": f"interop.runner/{__version__}",
        },
        "inputs_fingerprint": compute_inputs_fingerprint(),
        "cells": sorted(
            cells,
            key=lambda c: (c["case_id"], c["peer_id"], c["profile_id"]),
        ),
    }
    if args.write_results:
        write_json(RESULTS_FILE, payload)
        print(f"wrote {RESULTS_FILE}", file=sys.stderr)
    if args.generate_matrix and args.write_results:
        generate_matrix()
    return 0 if all(c["outcome"] != "fail" for c in cells) else 2


def _compose_project(case_id: str, peer_id: str) -> str:
    raw = f"ci-{case_id}-{peer_id}".lower().replace(".", "-")
    return "".join(c if c.isalnum() or c in "-_" else "-" for c in raw)[:63]


def _run_cell(
    *,
    case: Case,
    peer: Peer,
    profile_id: str,
    conduit_image: str,
    host_port: int,
    qname: str,
) -> tuple[str, str]:
    default_qname = qname.rstrip(".")
    default_qtype = "A"
    for oracle in case.oracles:
        if oracle.get("kind") == "fixture":
            from .paths import INTEROP, load_json

            path = INTEROP / oracle["path"]
            if path.is_file():
                expected = load_json(path)
                default_qname = expected.get("qname", default_qname).rstrip(".")
                default_qtype = str(expected.get("qtype") or default_qtype)
                break
    else:
        local_rr = case.peer_setup.get("local_rr") or []
        if local_rr and isinstance(local_rr[0], dict) and local_rr[0].get("name"):
            # Prefer the case's first local_rr over the CLI default smoke name.
            default_qname = str(local_rr[0]["name"]).rstrip(".")
            if local_rr[0].get("type"):
                default_qtype = str(local_rr[0]["type"])

    setup_ir = parse_peer_setup(case.peer_setup)
    stack = CellStack(
        conduit_image=conduit_image,
        peer=peer,
        profile_id=profile_id,
        setup_ir=setup_ir,
        conduit_delta=case.conduit_delta,
        conduit_assets=case.conduit_assets,
        host_port=host_port,
        project=_compose_project(case.id, peer.id),
    )
    try:
        stack.start()
        steps = case.queries or [{"qname": default_qname}]
        needs_parity = any(o.get("kind") == "parity" for o in case.oracles)
        via_steps: list = []
        direct = None
        for idx, step in enumerate(steps):
            step_q = str(step.get("qname", default_qname)).rstrip(".")
            step_t = str(step.get("qtype") or default_qtype or "A")
            bufsize = step.get("bufsize")
            dig_kw: dict = {"qtype": step_t}
            if bufsize is not None:
                dig_kw["bufsize"] = int(bufsize)
            if step.get("notcp"):
                dig_kw["notcp"] = True
            if step.get("ignore_tc"):
                dig_kw["ignore_tc"] = True
            if step.get("norecurse"):
                dig_kw["norecurse"] = True
            via = dig_query("127.0.0.1", host_port, step_q, **dig_kw)
            via_steps.append(via)
            if needs_parity:
                direct_host, direct_port = stack.peer_query_addr
                direct = dig_query(direct_host, direct_port, step_q, **dig_kw)
        outcome, detail = evaluate_oracles(
            case.oracles,
            via_conduit=via_steps[-1] if via_steps else None,
            via_steps=via_steps,
            direct=direct,
            peer_id=peer.id,
        )
        if outcome != "pass":
            return outcome, detail
        return "pass", f"{len(via_steps)} step(s): {detail}"
    finally:
        stack.stop()


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="interop.runner", description="DNSConduit interop harness")
    p.add_argument("--version", action="version", version=__version__)
    sub = p.add_subparsers(dest="command", required=True)

    fp = sub.add_parser("fingerprint", help="Print inputs fingerprint (PR gate)")
    fp.add_argument("-v", "--verbose", action="store_true")
    fp.set_defaults(func=cmd_fingerprint)

    ls = sub.add_parser("list", help="List peers and cases")
    ls.set_defaults(func=cmd_list)

    gm = sub.add_parser("generate-matrix", help="Write operator-docs matrix from results")
    gm.set_defaults(func=cmd_generate_matrix)

    run = sub.add_parser("run", help="Run filtered matrix cells")
    run.add_argument("--case", help="Case id")
    run.add_argument("--suite", choices=["smoke", "full"], help="Suite filter")
    run.add_argument("--peer", help="Peer id or product name")
    run.add_argument("--version", dest="version", help="Peer version filter")
    run.add_argument("--profile", default="forward-only", help="Conduit profile id")
    run.add_argument("--conduit-image", default="conduit:local", help="Conduit image ref")
    run.add_argument("--conduit-version", default="dev", help="Version label for provenance")
    run.add_argument("--host-port", type=int, default=15553, help="Host port for Conduit DNS")
    run.add_argument("--qname", default="www.smoke.test", help="Default query name")
    run.add_argument("--dry-run", action="store_true", help="Do not start Docker; record skip")
    run.add_argument("--write-results", action="store_true", help="Write interop/results/latest.json")
    run.add_argument("--merge", action="store_true", help="Merge cells into existing results")
    run.add_argument("--generate-matrix", action="store_true", help="Regenerate docs matrix after write")
    run.set_defaults(func=cmd_run)
    return p


def main(argv: Iterable[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
