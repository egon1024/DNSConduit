"""Materialize unbound.conf from SetupIR for the unbound family pack.

Recursive daemons must answer contract-case names deterministically without
touching the public internet. For each ``local_rr`` we declare its derived
zone as a static local-zone and add matching local-data — Unbound then
answers those names authoritatively from its own config, never recursing.
No root hints / forwarders are configured, so any other name simply fails
to resolve (acceptable: contract smoke cases only probe the local_rr names).
"""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import LocalRR, SetupIR
from interop.runner.zonegen import zone_name_for_record


def local_zone_conf_lines(rrs: list[LocalRR]) -> list[str]:
    """Render ``local-zone``/``local-data`` server-block lines for local_rr.

    One ``local-zone ... static`` per derived parent zone (deduplicated),
    followed by one ``local-data`` per record. Empty input renders no lines.
    """
    zones = sorted({zone_name_for_record(rr.name) for rr in rrs})
    lines = [f'local-zone: "{zone}." static' for zone in zones]
    for rr in rrs:
        name = rr.name.rstrip(".")
        lines.append(f'local-data: "{name} {rr.ttl} IN {rr.type} {rr.rdata}"')
    return lines


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    conf_lines = [
        "server:",
        "    interface: 0.0.0.0",
        "    port: 53",
        "    do-ip6: no",
        "    access-control: 0.0.0.0/0 allow",
        "    do-daemonize: no",
        "    verbosity: 1",
        '    chroot: ""',
        '    username: ""',
        '    directory: "/tmp"',
        '    logfile: ""',
        '    pidfile: ""',
    ]
    conf_lines.extend(f"    {line}" for line in local_zone_conf_lines(ir.local_rr))
    (out_dir / "unbound.conf").write_text("\n".join(conf_lines) + "\n", encoding="utf-8")
