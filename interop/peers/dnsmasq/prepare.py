"""Materialize dnsmasq run.sh from SetupIR (local_rr + optional fixtures as address=)."""

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
    for rr in ir.local_rr:
        if rr.type.upper() != "A":
            raise ValueError(f"dnsmasq pack only supports A local_rr in v1, got {rr.type}")
        name = rr.name.rstrip(".")
        args.append(f"--address=/{name}/{rr.rdata}")
    # Fixtures: not required for stub smoke; auth fixtures use auth families.
    run = out_dir / "run.sh"
    cmdline = " ".join(sh_quote(a) for a in args)
    run.write_text(f"#!/bin/sh\nexec {cmdline}\n", encoding="utf-8")
    run.chmod(0o755)


def sh_quote(s: str) -> str:
    return "'" + s.replace("'", "'\\''") + "'"
