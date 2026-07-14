"""Materialize knot.conf (+ zone files) from SetupIR for the knot family pack.

Serves synthetic zones derived from local_rr and/or fixture zones, whichever
(or both, or neither) the case's peer_setup supplies.
"""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import SetupIR
from interop.runner.zonegen import build_zone_plan


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    plan = build_zone_plan(ir, out_dir)
    zone_section = ""
    if plan:
        zone_stanzas = "\n".join(
            f"  - domain: {entry.zone_name}.\n    file: {entry.container_file}" for entry in plan
        )
        zone_section = f"zone:\n{zone_stanzas}\n"
    knot_conf = (
        "server:\n"
        "    listen: 0.0.0.0@53\n"
        "\n"
        "log:\n"
        "  - target: stdout\n"
        "    any: info\n"
        "\n"
        "database:\n"
        "    storage: /storage\n"
        "\n"
        f"{zone_section}"
    )
    (out_dir / "knot.conf").write_text(knot_conf, encoding="utf-8")
