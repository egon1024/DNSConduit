"""Materialize named.conf (+ zone files) from SetupIR for the bind family pack.

Serves synthetic zones derived from local_rr and/or fixture zones, whichever
(or both, or neither) the case's peer_setup supplies.
"""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import SetupIR
from interop.runner.zonegen import build_zone_plan


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    plan = build_zone_plan(ir, out_dir)
    zone_stanzas = "\n".join(
        f'zone "{entry.zone_name}" {{\n'
        f"    type master;\n"
        f'    file "{entry.container_file}";\n'
        f"}};"
        for entry in plan
    )
    named_conf = (
        "options {\n"
        '    directory "/tmp";\n'
        "    recursion no;\n"
        "    allow-query { any; };\n"
        "    listen-on { any; };\n"
        "    listen-on-v6 { any; };\n"
        "    pid-file none;\n"
        "};\n"
        "\n"
        f"{zone_stanzas}\n"
    )
    (out_dir / "named.conf").write_text(named_conf, encoding="utf-8")
