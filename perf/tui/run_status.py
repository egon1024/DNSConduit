"""Run timing / progress helpers for the TUI status panel."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from typing import Literal

from perf.runner.api import RunProgressEvent

RunPhase = Literal["idle", "running", "complete", "failed", "cancelled"]


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


def format_clock(dt: datetime | None) -> str:
    if dt is None:
        return "—"
    local = dt.astimezone()
    return local.strftime("%H:%M:%S")


def format_duration(seconds: float | None) -> str:
    if seconds is None or seconds < 0:
        return "—"
    seconds = int(round(seconds))
    h, rem = divmod(seconds, 3600)
    m, s = divmod(rem, 60)
    if h:
        return f"{h}h {m:02d}m {s:02d}s"
    if m:
        return f"{m}m {s:02d}s"
    return f"{s}s"


@dataclass
class RunStatusModel:
    phase: RunPhase = "idle"
    started_at: datetime | None = None
    ended_at: datetime | None = None
    cycles: int = 1
    scenarios_per_cycle: int = 0
    completed_units: int = 0
    current_cycle: int = 0
    current_scenario: str = ""
    detail: str = "No run yet"
    planned_cell_seconds: float | None = None  # load + warmup guess per cell
    error_message: str = ""

    def total_units(self) -> int:
        return max(self.cycles * max(self.scenarios_per_cycle, 1), 1)

    def fraction(self) -> float:
        if self.phase == "complete":
            return 1.0
        if self.phase != "running":
            return 0.0
        return min(1.0, self.completed_units / float(self.total_units()))

    def elapsed_seconds(self, now: datetime | None = None) -> float | None:
        if self.started_at is None:
            return None
        end = self.ended_at or (now or _utc_now())
        return max(0.0, (end - self.started_at).total_seconds())

    def eta_at(self, now: datetime | None = None) -> datetime | None:
        if self.phase != "running" or self.started_at is None:
            return None
        now = now or _utc_now()
        frac = self.fraction()
        elapsed = self.elapsed_seconds(now) or 0.0
        # Prefer measured pace once at least one cell finished.
        if self.completed_units >= 1 and frac > 0:
            total_est = elapsed / frac
            remaining = max(0.0, total_est - elapsed)
            return now + timedelta(seconds=remaining)
        # Fallback: planned cell duration × remaining units.
        if self.planned_cell_seconds and self.scenarios_per_cycle:
            remaining_units = max(0, self.total_units() - self.completed_units)
            # If mid-cell, count current as still remaining.
            return now + timedelta(seconds=remaining_units * self.planned_cell_seconds)
        return None

    def mark_start(
        self,
        *,
        cycles: int,
        scenarios_per_cycle: int,
        planned_cell_seconds: float | None,
        detail: str,
    ) -> None:
        self.phase = "running"
        self.started_at = _utc_now()
        self.ended_at = None
        self.cycles = max(1, cycles)
        self.scenarios_per_cycle = max(0, scenarios_per_cycle)
        self.completed_units = 0
        self.current_cycle = 1
        self.current_scenario = ""
        self.planned_cell_seconds = planned_cell_seconds
        self.detail = detail
        self.error_message = ""

    def observe(self, event: RunProgressEvent) -> None:
        if self.phase != "running":
            return
        if event.cycles:
            self.cycles = event.cycles
        if event.total:
            self.scenarios_per_cycle = event.total
        if event.kind == "cycle_start":
            self.current_cycle = event.cycle
            self.detail = f"Cycle {event.cycle}/{event.cycles}"
        elif event.kind == "scenario_start":
            self.current_cycle = event.cycle
            self.current_scenario = event.scenario_id or ""
            self.detail = (
                f"Cycle {event.cycle}/{event.cycles} · "
                f"{event.index}/{event.total} · {self.current_scenario}"
            )
        elif event.kind == "scenario_done":
            # Units completed through this scenario in this cycle.
            self.completed_units = (event.cycle - 1) * max(event.total, 1) + event.index
            self.detail = (
                f"Finished {event.scenario_id or 'scenario'} "
                f"({event.index}/{event.total}, cycle {event.cycle}/{event.cycles})"
            )
        elif event.kind == "cancelled":
            self.phase = "cancelled"
            self.ended_at = _utc_now()
            self.detail = event.message or "Cancelled"
        elif event.kind == "message" and event.message:
            # Keep last operational detail short.
            if event.message.startswith("warning:") or event.message.startswith(
                "killed "
            ):
                self.detail = event.message[:120]

    def mark_complete(self, *, ok: bool, detail: str = "") -> None:
        self.ended_at = _utc_now()
        if self.phase == "cancelled":
            return
        if ok:
            self.phase = "complete"
            self.completed_units = self.total_units()
            self.detail = detail or "Run complete"
            self.current_scenario = ""
        else:
            self.phase = "failed"
            self.detail = detail or self.error_message or "Run failed"
            self.error_message = self.detail

    def status_label(self) -> str:
        return {
            "idle": "Idle",
            "running": "Running",
            "complete": "Complete",
            "failed": "Failed",
            "cancelled": "Cancelled",
        }[self.phase]

    def render_lines(self, now: datetime | None = None) -> tuple[str, str, str]:
        """Return (headline, times_line, detail_line)."""
        now = now or _utc_now()
        headline = f"{self.status_label()}"
        if self.phase == "running" and self.current_scenario:
            headline += f"  ·  {self.current_scenario}"
        elif self.phase == "running":
            headline += f"  ·  cycle {self.current_cycle}/{self.cycles}"

        elapsed = format_duration(self.elapsed_seconds(now))
        start = format_clock(self.started_at)
        end = format_clock(self.ended_at)
        if self.phase == "running":
            eta = self.eta_at(now)
            eta_s = format_clock(eta) if eta else "—"
            times = f"Started {start}   Elapsed {elapsed}   ETA ~{eta_s}"
        elif self.phase in ("complete", "failed", "cancelled"):
            times = f"Started {start}   Ended {end}   Duration {elapsed}"
        else:
            times = "Started —   Ended —   Duration —"

        progress = ""
        if self.scenarios_per_cycle or self.phase == "running":
            progress = (
                f"{self.completed_units}/{self.total_units()} cells  ·  {self.detail}"
            )
        else:
            progress = self.detail
        return headline, times, progress
