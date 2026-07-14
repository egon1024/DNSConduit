"""Product-neutral peer_setup IR."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .paths import FIXTURES


@dataclass(frozen=True)
class LocalRR:
    name: str
    type: str
    rdata: str
    ttl: int = 300


@dataclass(frozen=True)
class SetupIR:
    fixtures: list[str] = field(default_factory=list)
    local_rr: list[LocalRR] = field(default_factory=list)
    # Domains handled locally by the stub (dnsmasq --local=/zone/) → NXDOMAIN when
    # no matching local_rr; ignored by auth packs that load fixtures/zone files.
    local_zones: list[str] = field(default_factory=list)


def parse_peer_setup(raw: dict[str, Any] | None) -> SetupIR:
    if not raw:
        return SetupIR()
    rrs: list[LocalRR] = []
    for item in raw.get("local_rr") or []:
        rrs.append(
            LocalRR(
                name=str(item["name"]),
                type=str(item["type"]),
                rdata=str(item["rdata"]),
                ttl=int(item.get("ttl", 300)),
            )
        )
    zones = [str(z) for z in (raw.get("local_zones") or [])]
    return SetupIR(
        fixtures=[str(x) for x in (raw.get("fixtures") or [])],
        local_rr=rrs,
        local_zones=zones,
    )


def resolve_fixture_dirs(ir: SetupIR, fixtures_root: Path = FIXTURES) -> list[Path]:
    found: list[Path] = []
    for zone_id in ir.fixtures:
        path = fixtures_root / "zones" / zone_id
        if not path.is_dir():
            raise FileNotFoundError(f"fixture zone missing: {zone_id} ({path})")
        found.append(path)
    return found
