"""Materialize named.conf (+ zone files) for the bind-recursive family pack.

Recursive daemons must answer contract-case names deterministically without
touching the public internet. Synthetic master zones are built from local_rr
(same zonegen path as auth BIND). ``recursion yes`` with empty ``forward only``
forwarders keeps exterior lookups from using built-in root hints; other names
may SERVFAIL — acceptable for contract smoke, which only probes local_rr names.
"""

from __future__ import annotations

from pathlib import Path

from interop.runner.setup_ir import SetupIR
from interop.runner.zonegen import build_zone_plan, render_named_zone_stanzas


def prepare(*, out_dir: Path, ir: SetupIR, peer) -> None:
    plan = build_zone_plan(ir, out_dir)
    zone_stanzas = render_named_zone_stanzas(plan)
    named_conf = (
        "options {\n"
        '    directory "/tmp";\n'
        "    recursion yes;\n"
        "    allow-recursion { any; };\n"
        "    allow-query { any; };\n"
        "    dnssec-validation no;\n"
        "    forwarders { };\n"
        "    forward only;\n"
        "    listen-on { any; };\n"
        "    listen-on-v6 { any; };\n"
        "    pid-file none;\n"
        "};\n"
        "\n"
        f"{zone_stanzas}\n"
    )
    (out_dir / "named.conf").write_text(named_conf, encoding="utf-8")
