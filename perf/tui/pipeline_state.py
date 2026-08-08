"""Hybrid pipeline sync state for the perf lifecycle TUI."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Literal

from perf.runner.api import (
    file_fingerprint,
    read_source_reference_stamp,
    resolve_latest_reference_path,
)

SyncStatus = Literal["in_sync", "stale", "unknown"]


def _utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def compare_fingerprints(expected: str | None, actual: str | None) -> SyncStatus:
    if expected is None or actual is None:
        return "unknown"
    return "in_sync" if expected == actual else "stale"


def fingerprints_for_paths(paths: list[Path]) -> list[str | None]:
    return [file_fingerprint(p) for p in paths]


@dataclass
class PipelineState:
    """Durable file identity + light in-session hints."""

    last_run_paths: list[Path] = field(default_factory=list)
    last_run_fps: list[str | None] = field(default_factory=list)
    last_merge_path: Path | None = None
    last_merge_fp: str | None = None
    last_promote_path: Path | None = None
    last_promote_fp: str | None = None
    session_hint: dict[str, str] = field(default_factory=dict)

    def note(self, stage: str, message: str) -> None:
        self.session_hint[stage] = f"{message} ({_utc_now()})"

    def record_runs(self, paths: list[Path]) -> None:
        self.last_run_paths = list(paths)
        self.last_run_fps = fingerprints_for_paths(paths)
        self.note("run", f"completed {len(paths)} cycle(s)")

    def record_merge(self, path: Path) -> None:
        self.last_merge_path = path
        self.last_merge_fp = file_fingerprint(path)
        self.note("merge", f"merged → {path}")

    def record_promote(self, path: Path) -> None:
        self.last_promote_path = path
        self.last_promote_fp = file_fingerprint(path)
        self.note("promote", f"promoted → {path}")

    def record_generate(self) -> None:
        self.note("generate", "docs generated")

    def merge_sync(self) -> SyncStatus:
        """Merge inputs vs last Run outputs (when merge sources == last runs)."""
        if not self.last_run_paths or self.last_merge_path is None:
            return "unknown"
        # Merge is in sync with run if we still have the same run fingerprints
        # that were present when merge was recorded — approximate: merge file
        # exists and runs still match recorded fingerprints.
        current = fingerprints_for_paths(self.last_run_paths)
        if any(fp is None for fp in current) or any(
            fp is None for fp in self.last_run_fps
        ):
            return "unknown"
        if current != self.last_run_fps:
            return "stale"
        if file_fingerprint(self.last_merge_path) != self.last_merge_fp:
            return "stale"
        return "in_sync"

    def promote_sync(self) -> SyncStatus:
        if self.last_merge_path is None or self.last_promote_path is None:
            return "unknown"
        merge_now = file_fingerprint(self.last_merge_path)
        if merge_now is None or self.last_merge_fp is None:
            return "unknown"
        if merge_now != self.last_merge_fp:
            return "stale"
        promo_now = file_fingerprint(self.last_promote_path)
        if promo_now is None or self.last_promote_fp is None:
            return "unknown"
        return "in_sync" if promo_now == self.last_promote_fp else "stale"

    def generate_sync(self) -> SyncStatus:
        """Docs in sync when stamp fingerprint matches current promoted reference."""
        latest = resolve_latest_reference_path()
        if latest is None:
            return "unknown"
        latest_fp = file_fingerprint(latest)
        stamp = read_source_reference_stamp()
        if stamp is None or latest_fp is None:
            return "unknown"
        stamped = stamp.get("fingerprint")
        if not isinstance(stamped, str):
            return "unknown"
        return compare_fingerprints(latest_fp, stamped)

    def run_sync(self) -> SyncStatus:
        if not self.last_run_paths:
            return "unknown"
        current = fingerprints_for_paths(self.last_run_paths)
        if any(fp is None for fp in current):
            return "unknown"
        return "in_sync" if current == self.last_run_fps else "stale"

    def badge(self, stage: str) -> SyncStatus:
        if stage == "run":
            return self.run_sync()
        if stage == "merge_promote":
            merge_s = self.merge_sync()
            promo_s = self.promote_sync()
            if merge_s == "stale" or promo_s == "stale":
                return "stale"
            if merge_s == "in_sync" and promo_s == "in_sync":
                return "in_sync"
            if merge_s == "unknown" and promo_s == "unknown":
                return "unknown"
            if "in_sync" in (merge_s, promo_s) and "stale" not in (merge_s, promo_s):
                # partial progress
                return "in_sync" if merge_s == "in_sync" and promo_s == "unknown" else "unknown"
            return "unknown"
        if stage == "generate":
            return self.generate_sync()
        return "unknown"


def sync_label(status: SyncStatus) -> str:
    return {
        "in_sync": "in sync",
        "stale": "stale",
        "unknown": "unknown",
    }[status]


# Re-export for tests
__all__ = [
    "PipelineState",
    "SyncStatus",
    "compare_fingerprints",
    "sync_label",
]
