"""Materialize dnsmasq run.sh from SetupIR (local_rr as addn-hosts + optional local zones)."""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import SetupIR


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    args: list[str] = [
        "dnsmasq",
        "-k",
        "--no-daemon",
        "--log-queries",
        "--port=53",
        "--bind-interfaces",
        "--interface=eth0",
        "--except-interface=lo",
    ]
    if ir.local_rr:
        # addn-hosts preserves multi-A RRsets for the same name; repeated --address=
        # keeps only the last address for that domain.
        hosts = out_dir / "hosts"
        lines: list[str] = []
        for rr in ir.local_rr:
            if rr.type.upper() != "A":
                raise ValueError(f"dnsmasq pack only supports A local_rr in v1, got {rr.type}")
            name = rr.name.rstrip(".")
            lines.append(f"{rr.rdata} {name}\n")
        hosts.write_text("".join(lines), encoding="utf-8")
        args.append("--addn-hosts=/peer-config/hosts")
    for zone in ir.local_zones:
        # Authoritative/local: unanswered names under the zone return NXDOMAIN.
        z = zone.strip().strip(".")
        if z:
            args.append(f"--local=/{z}/")
    # Fixtures: not required for stub smoke; auth fixtures use auth families.
    run = out_dir / "run.sh"
    cmdline = " ".join(sh_quote(a) for a in args)
    run.write_text(f"#!/bin/sh\nexec {cmdline}\n", encoding="utf-8")
    run.chmod(0o755)


def sh_quote(s: str) -> str:
    return "'" + s.replace("'", "'\\''") + "'"
