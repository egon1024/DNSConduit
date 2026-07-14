"""Materialize pdns.conf + bind-config from SetupIR for the pdns-auth family pack.

Uses PowerDNS's "bind" backend so the zone list is a plain BIND-style
named.conf zone clause file; serves synthetic zones derived from local_rr
and/or fixture zones, whichever (or both, or neither) peer_setup supplies.
"""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import SetupIR
from interop.runner.zonegen import build_zone_plan, render_named_zone_stanzas


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    plan = build_zone_plan(ir, out_dir)
    zone_stanzas = render_named_zone_stanzas(plan)
    (out_dir / "named.conf").write_text(zone_stanzas + "\n", encoding="utf-8")

    pdns_conf = (
        "local-address=0.0.0.0\n"
        "local-port=53\n"
        "launch=bind\n"
        "bind-config=/peer-config/named.conf\n"
        "disable-syslog=yes\n"
        "daemon=no\n"
        "guardian=no\n"
    )
    (out_dir / "pdns.conf").write_text(pdns_conf, encoding="utf-8")
