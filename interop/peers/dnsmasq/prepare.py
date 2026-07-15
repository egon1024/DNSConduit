"""Materialize dnsmasq run.sh from SetupIR (local_rr as addn-hosts + optional local zones + CNAME)."""

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
    hosts_lines: list[str] = []
    cnames: list[tuple[str, str]] = []
    for rr in ir.local_rr:
        rtype = rr.type.upper()
        name = rr.name.rstrip(".")
        if rtype == "A":
            # addn-hosts preserves multi-A RRsets for the same name; repeated --address=
            # keeps only the last address for that domain.
            hosts_lines.append(f"{rr.rdata} {name}\n")
        elif rtype == "CNAME":
            # --cname=alias,target — target must also be known locally (hosts/DHCP).
            target = rr.rdata.rstrip(".")
            cnames.append((name, target))
        else:
            raise ValueError(f"dnsmasq pack only supports A and CNAME local_rr in v1, got {rr.type}")
    if hosts_lines:
        hosts = out_dir / "hosts"
        hosts.write_text("".join(hosts_lines), encoding="utf-8")
        args.append("--addn-hosts=/peer-config/hosts")
    for alias, target in cnames:
        args.append(f"--cname={alias},{target}")
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
