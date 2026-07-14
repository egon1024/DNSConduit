"""Shared BIND-format zone synthesis for auth peer family packs.

Auth peers (bind, knot, pdns-auth) must serve two kinds of zones:

- synthetic zones derived from a case's ``local_rr`` (e.g. ``www.smoke.test.``
  -> zone ``smoke.test``), used by cross-role smoke/parity cases.
- fixture zones copied verbatim from ``interop/fixtures/zones/<id>/`` for
  fixture-oracle cases.

This module only covers the synthetic side; fixture zone files are already
materialized by ``peer_packs._copy_fixtures`` and are located with
``find_fixture_zone_file``.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from .setup_ir import LocalRR, SetupIR

CONTAINER_MOUNT = "/peer-config"
"""Container path where ``compose.override.yml`` mounts ``${PEER_CONFIG_DIR}`` (ro).

All auth family packs (bind, knot, pdns-auth) mount the materialized ``out_dir``
at this fixed path, matching the convention already used by the dnsmasq pack.
"""


def zone_name_for_record(name: str) -> str:
    """Derive the parent zone for a local_rr owner name.

    Strips the leftmost label, e.g. ``"www.smoke.test."`` -> ``"smoke.test"``.
    """
    labels = name.rstrip(".").split(".")
    if len(labels) < 2:
        raise ValueError(f"local_rr name {name!r} has no parent zone to derive")
    return ".".join(labels[1:])


@dataclass(frozen=True)
class SyntheticZone:
    name: str
    records: list[LocalRR]


def group_local_rr_by_zone(rrs: list[LocalRR]) -> list[SyntheticZone]:
    """Group local_rr entries by their derived parent zone, sorted by zone name."""
    by_zone: dict[str, list[LocalRR]] = {}
    for rr in rrs:
        zone = zone_name_for_record(rr.name)
        by_zone.setdefault(zone, []).append(rr)
    return [SyntheticZone(name=zone, records=recs) for zone, recs in sorted(by_zone.items())]


def _owner_for(rr_name: str, zone_name: str) -> str:
    rel = rr_name.rstrip(".")
    if rel == zone_name:
        return "@"
    suffix = "." + zone_name
    if rel.endswith(suffix):
        return rel[: -len(suffix)]
    raise ValueError(f"local_rr {rr_name!r} does not belong to zone {zone_name!r}")


def render_zone_file(zone: SyntheticZone, serial: int = 2026071301) -> str:
    """Render a standard RFC 1035 zone file (SOA + NS + local_rr records)."""
    lines = [
        f"; Harness-synthesized zone for auth peer packs (from case local_rr).",
        "$TTL 300",
        f"@\tIN SOA ns.{zone.name}. hostmaster.{zone.name}. (",
        f"\t\t{serial} ; serial",
        "\t\t3600       ; refresh",
        "\t\t600        ; retry",
        "\t\t86400      ; expire",
        "\t\t300 )      ; minimum",
        f"\tIN NS  ns.{zone.name}.",
        "ns\tIN A   192.0.2.1",
    ]
    for rr in zone.records:
        owner = _owner_for(rr.name, zone.name)
        lines.append(f"{owner}\t{rr.ttl}\tIN {rr.type}\t{rr.rdata}")
    return "\n".join(lines) + "\n"


def write_synthetic_zones(rrs: list[LocalRR], out_dir: Path) -> list[tuple[SyntheticZone, Path]]:
    """Write one zone file per derived zone under ``out_dir``; return (zone, path) pairs."""
    zones = group_local_rr_by_zone(rrs)
    out_dir.mkdir(parents=True, exist_ok=True)
    written: list[tuple[SyntheticZone, Path]] = []
    for zone in zones:
        path = out_dir / f"{zone.name}.zone"
        path.write_text(render_zone_file(zone), encoding="utf-8")
        written.append((zone, path))
    return written


def find_fixture_zone_file(zone_dir: Path, zone_id: str) -> Path:
    """Locate the BIND-format zone file within a copied fixture zone directory.

    Fixture directories may also contain oracle files (e.g. ``expected-a.json``);
    the zone file follows the ``db.<zone-id>`` convention used under
    ``interop/fixtures/zones/<zone-id>/``.
    """
    conventional = zone_dir / f"db.{zone_id}"
    if conventional.is_file():
        return conventional
    candidates = [p for p in zone_dir.iterdir() if p.is_file() and p.suffix != ".json"]
    if len(candidates) == 1:
        return candidates[0]
    raise FileNotFoundError(
        f"cannot locate zone file for fixture {zone_id!r} in {zone_dir} "
        f"(expected db.{zone_id} or exactly one non-JSON file)"
    )


def container_path(out_dir: Path, path: Path) -> str:
    """Map a host path under ``out_dir`` to its container-mounted equivalent."""
    rel = path.relative_to(out_dir)
    return f"{CONTAINER_MOUNT}/{rel.as_posix()}"


@dataclass(frozen=True)
class ZonePlanEntry:
    zone_name: str
    container_file: str


def render_named_zone_stanzas(plan: list[ZonePlanEntry]) -> str:
    """Render BIND-style ``zone "<name>" { type master; file "<path>"; };`` clauses.

    Shared by the bind and pdns-auth (bind backend) packs so the stanza format
    and the single point of file/name interpolation stay in one place. Returns
    an empty string for an empty plan (valid: an auth daemon with no zones).
    """
    return "\n".join(
        f'zone "{entry.zone_name}" {{\n'
        f"    type master;\n"
        f'    file "{entry.container_file}";\n'
        f"}};"
        for entry in plan
    )


def build_zone_plan(ir: SetupIR, out_dir: Path) -> list[ZonePlanEntry]:
    """Build the full auth zone plan: synthetic zones (from local_rr) plus fixtures.

    Handles both present gracefully: no local_rr -> no synthetic zones; no
    fixtures -> no fixture zones. Callers (bind/knot/pdns-auth prepare.py) turn
    this into daemon-specific zone stanzas.
    """
    plan: list[ZonePlanEntry] = []
    for zone, path in write_synthetic_zones(ir.local_rr, out_dir / "synth"):
        plan.append(ZonePlanEntry(zone_name=zone.name, container_file=container_path(out_dir, path)))
    for zone_id in ir.fixtures:
        zone_dir = out_dir / "zones" / zone_id
        zone_file = find_fixture_zone_file(zone_dir, zone_id)
        plan.append(ZonePlanEntry(zone_name=zone_id, container_file=container_path(out_dir, zone_file)))
    return plan
