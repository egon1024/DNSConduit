"""Shared paths and YAML/JSON helpers for the performance harness."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

try:
    import yaml
except ImportError as exc:  # pragma: no cover
    raise SystemExit(
        "PyYAML is required for perf.runner (pip install -r perf/requirements.txt)"
    ) from exc

ROOT = Path(__file__).resolve().parents[2]
PERF = ROOT / "perf"
CATALOG = PERF / "catalog"
SCENARIOS = CATALOG / "scenarios"
LAB_PROFILES = CATALOG / "lab_profiles"
ANNOTATIONS = CATALOG / "annotations"
FIXTURES = PERF / "fixtures"
CONFIGS = FIXTURES / "configs"
QUERIES = FIXTURES / "queries"
UPSTREAM = FIXTURES / "upstream"
DNSPERF_DIR = FIXTURES / "dnsperf"
RESULTS = PERF / "results"
RESULTS_SCHEMA = RESULTS / "schema.json"
RUNS_DIR = RESULTS / "runs"
REFERENCES_DIR = RESULTS / "references"


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
        json.dump(data, f, indent=2, sort_keys=False)
        f.write("\n")
