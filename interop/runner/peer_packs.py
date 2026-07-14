"""Resolve and materialize interop/peers/<family> packs."""

from __future__ import annotations

import importlib.util
import shutil
from pathlib import Path
from string import Template
from typing import Any

from .paths import PEERS_PACKS
from .setup_ir import SetupIR, resolve_fixture_dirs


def pack_dir_for_family(family: str) -> Path:
    path = PEERS_PACKS / family
    if not path.is_dir():
        raise FileNotFoundError(f"peer family pack missing: {family} ({path})")
    return path


def _render_templates(templates_dir: Path, out_dir: Path, mapping: dict[str, str]) -> None:
    if not templates_dir.is_dir():
        return
    for src in templates_dir.rglob("*"):
        if not src.is_file():
            continue
        rel = src.relative_to(templates_dir)
        dest = out_dir / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        text = src.read_text(encoding="utf-8")
        dest.write_text(Template(text).safe_substitute(mapping), encoding="utf-8")


def _copy_fixtures(ir: SetupIR, out_zones: Path) -> list[Path]:
    out_zones.mkdir(parents=True, exist_ok=True)
    copied: list[Path] = []
    for zone_dir in resolve_fixture_dirs(ir):
        dest = out_zones / zone_dir.name
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(zone_dir, dest)
        copied.append(dest)
    return copied


def _run_prepare(pack: Path, out_dir: Path, ir: SetupIR, peer: Any) -> None:
    prepare = pack / "prepare.py"
    if not prepare.is_file():
        return
    spec = importlib.util.spec_from_file_location(f"interop_peer_prepare_{pack.name}", prepare)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {prepare}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    if hasattr(mod, "prepare"):
        mod.prepare(out_dir=out_dir, ir=ir, peer=peer)


def materialize_peer_config(
    *,
    family: str,
    ir: SetupIR,
    out_dir: Path,
    peer: Any,
    mapping_extra: dict[str, str] | None = None,
) -> Path:
    """
    Render family pack into out_dir. Returns path to compose.override.yml to layer.
    """
    pack = pack_dir_for_family(family)
    out_dir.mkdir(parents=True, exist_ok=True)
    zones = _copy_fixtures(ir, out_dir / "zones")
    local_lines = "\n".join(
        f"{rr.name} {rr.ttl} IN {rr.type} {rr.rdata}" for rr in ir.local_rr
    )
    mapping = {
        "LOCAL_RR_BIND_LINES": local_lines,
        "FIXTURE_ZONE_IDS": ",".join(ir.fixtures),
        "CONFIG_DIR": str(out_dir),
        **(mapping_extra or {}),
    }
    _render_templates(pack / "templates", out_dir, mapping)
    _run_prepare(pack, out_dir, ir, peer)
    override = pack / "compose.override.yml"
    if not override.is_file():
        raise FileNotFoundError(f"family pack missing compose.override.yml: {family}")
    (out_dir / ".pack_override").write_text(str(override.resolve()), encoding="utf-8")
    (out_dir / ".fixture_zones").write_text(
        "\n".join(str(p) for p in zones), encoding="utf-8"
    )
    return override
