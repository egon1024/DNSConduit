"""Build, validate, and write canonical run JSON documents."""

from __future__ import annotations

import platform
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .paths import RESULTS_SCHEMA, RUNS_DIR, load_json, write_json

try:
    import jsonschema
except ImportError:  # pragma: no cover
    jsonschema = None  # type: ignore


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def detect_meminfo_total_mb() -> int | None:
    """Return MemTotal from /proc/meminfo in whole MiB, or None if unavailable."""
    try:
        text = Path("/proc/meminfo").read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    for line in text.splitlines():
        if line.startswith("MemTotal:"):
            parts = line.split()
            if len(parts) >= 2 and parts[1].isdigit():
                # Kernel reports KiB; store whole MiB for a stable, compact field.
                return int(parts[1]) // 1024
            return None
    return None


def detect_lab_profile_runtime(
    *,
    profile_id: str = "local",
    display_name: str | None = None,
) -> dict[str, Any]:
    """Best-effort host facts for local runs (not the filled reference-profile template)."""
    cpu = platform.processor() or platform.machine() or "unknown"
    # Prefer /proc/cpuinfo model name on Linux.
    try:
        text = Path("/proc/cpuinfo").read_text(encoding="utf-8", errors="replace")
        for line in text.splitlines():
            if line.lower().startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass

    logical = os_cpu_count()
    profile: dict[str, Any] = {
        "id": profile_id,
        "display_name": display_name or f"Local run ({profile_id})",
        "cpu_model": cpu,
        "physical_cores": logical,  # accurate physical count is optional
        "logical_cores": logical,
        "os": f"{platform.system()} {platform.release()}",
        "kernel": platform.release(),
    }
    mem_mb = detect_meminfo_total_mb()
    if mem_mb is not None:
        profile["memory_total_mb"] = mem_mb
    return profile


def os_cpu_count() -> int:
    import os

    return os.cpu_count() or 1


def validate_run_document(doc: dict[str, Any], schema_path: Path = RESULTS_SCHEMA) -> None:
    if "lab_profile" not in doc:
        raise ValueError("run document missing required lab_profile")
    profile = doc["lab_profile"]
    if not profile.get("id") or not profile.get("cpu_model"):
        raise ValueError(
            "lab_profile must include id and cpu_model (refusing to write invalid run)"
        )
    if jsonschema is None:
        # Lightweight fallback when jsonschema is not installed.
        for key in ("schema_version", "generated_at", "provenance", "scenarios"):
            if key not in doc:
                raise ValueError(f"run document missing required field: {key}")
        return
    schema = load_json(schema_path)
    jsonschema.validate(instance=doc, schema=schema)


def write_run_document(
    doc: dict[str, Any],
    *,
    path: Path | None = None,
    validate: bool = True,
) -> Path:
    if validate:
        validate_run_document(doc)
    if path is None:
        stamp = re.sub(r"[^0-9TZ]", "", doc.get("generated_at", utc_now_iso()))
        path = RUNS_DIR / f"run-{stamp}.json"
    write_json(path, doc)
    return path
