"""Shared paths and YAML/JSON helpers for the interop runner."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

try:
    import yaml
except ImportError as exc:  # pragma: no cover
    raise SystemExit(
        "PyYAML is required for interop.runner (pip install pyyaml)"
    ) from exc

ROOT = Path(__file__).resolve().parents[2]
INTEROP = ROOT / "interop"
CATALOG = INTEROP / "catalog"
CASES = CATALOG / "cases"
PROFILES = CATALOG / "profiles"
PEERS_FILE = CATALOG / "peers.yaml"
RESULTS_FILE = INTEROP / "results" / "latest.json"
RESULTS_SCHEMA = INTEROP / "results" / "schema.json"
COMPOSE_CELL = INTEROP / "compose" / "cell.compose.yml"
FIXTURES = INTEROP / "fixtures"
PEERS_PACKS = INTEROP / "peers"


def load_yaml(path: Path) -> Any:
    with path.open(encoding="utf-8") as f:
        return yaml.safe_load(f)


def write_yaml(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        yaml.safe_dump(data, f, sort_keys=False, default_flow_style=False)


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as f:
        return json.load(f)


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, sort_keys=True)
        f.write("\n")
