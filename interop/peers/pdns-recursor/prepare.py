"""Materialize recursor.conf (+ zone files) from SetupIR for the pdns-recursor pack.

Recursive daemons must answer contract-case names deterministically without
touching the public internet. Rather than a Lua hook, we reuse the shared
auth-zone synthesis (``zonegen.build_zone_plan``) and declare each derived
zone via Recursor's ``auth-zones`` setting: the zone becomes locally
authoritative, so the recursor answers those names straight from the zone
file and never recurses for them. Other names may legitimately SERVFAIL /
attempt real recursion — acceptable for contract smoke, which only probes
the local_rr names.

Recursor 5.x defaults to YAML settings; classic ``key=value`` config (still
used here, matching the other family packs' plain-text style) requires
``--enable-old-settings=yes`` on the command line (set in
``compose.override.yml``).
"""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import SetupIR
from interop.runner.zonegen import build_zone_plan


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    plan = build_zone_plan(ir, out_dir)
    lines = [
        "local-address=0.0.0.0",
        "local-port=53",
        "allow-from=0.0.0.0/0",
        "daemon=no",
        "disable-syslog=yes",
        "socket-dir=/tmp",
        "write-pid=no",
    ]
    if plan:
        auth_zones = ",".join(f"{entry.zone_name}={entry.container_file}" for entry in plan)
        lines.append(f"auth-zones={auth_zones}")
    (out_dir / "recursor.conf").write_text("\n".join(lines) + "\n", encoding="utf-8")
