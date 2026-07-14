"""Merge Conduit profile with case conduit_delta (replace listed top-level keys)."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .paths import load_yaml, write_yaml


def merge_conduit_profile(
    profile_path: Path,
    delta: dict[str, Any] | None,
    out_path: Path,
) -> dict[str, Any]:
    data = dict(load_yaml(profile_path) or {})
    if delta:
        for key, value in delta.items():
            data[key] = value
    write_yaml(out_path, data)
    return data
