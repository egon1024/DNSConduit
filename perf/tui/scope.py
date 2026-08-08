"""Shared catalog scope selection for the Run stage."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from perf.runner.api import list_scenario_summaries, list_study_summaries
from perf.runner.paths import ROOT


@dataclass
class ScopeSelection:
    """What the Run stage will pass to ``run_benchmarks``."""

    publish_set: bool = False
    curated_only: bool = False
    study_ids: list[str] = field(default_factory=list)
    suites: list[str] = field(default_factory=list)
    scenario_ids: list[str] = field(default_factory=list)

    def summary(self) -> str:
        if self.publish_set:
            return "Publish set (all published study members)"
        parts: list[str] = []
        if self.curated_only and not (
            self.study_ids or self.suites or self.scenario_ids
        ):
            return "Curated scenarios only"
        if self.study_ids:
            parts.append(
                "studies: " + ", ".join(self.study_ids[:4])
                + ("…" if len(self.study_ids) > 4 else "")
            )
        if self.suites:
            parts.append("suites: " + ", ".join(self.suites))
        if self.scenario_ids:
            parts.append(
                f"{len(self.scenario_ids)} scenario(s)"
                + (
                    f" ({', '.join(self.scenario_ids[:3])}…)"
                    if len(self.scenario_ids) > 3
                    else f" ({', '.join(self.scenario_ids)})"
                )
            )
        if self.curated_only:
            parts.append("curated filter")
        return "; ".join(parts) if parts else "(none selected — choose a scope)"

    def is_empty(self) -> bool:
        return not (
            self.publish_set
            or self.curated_only
            or self.study_ids
            or self.suites
            or self.scenario_ids
        )

    def to_run_kwargs(self) -> dict:
        kwargs: dict = {}
        if self.publish_set:
            kwargs["publish_set"] = True
            if self.scenario_ids:
                kwargs["scenario_ids"] = list(self.scenario_ids)
            return kwargs
        if self.study_ids:
            kwargs["study_ids"] = list(self.study_ids)
        if self.suites:
            kwargs["suites"] = list(self.suites)
        if self.scenario_ids:
            kwargs["scenario_ids"] = list(self.scenario_ids)
        if self.curated_only:
            kwargs["curated_only"] = True
        return kwargs


def default_scope() -> ScopeSelection:
    """Publish-set: all published study members (operator-docs reference spine)."""
    return ScopeSelection(publish_set=True)


def default_run_profile() -> str:
    return "maintainer-ws-1"


def default_run_cycles() -> str:
    return "3"


def default_run_time() -> str:
    """Empty = harness/scenario default duration (publish-quality)."""
    return ""


def default_conduit_path() -> str:
    for rel in ("target/release/conduit", "target/debug/conduit"):
        path = ROOT / rel
        if path.is_file():
            return str(path)
    return ""


def catalog_suites() -> list[str]:
    suites = sorted({suite for _id, suite, _c, _s in list_scenario_summaries()})
    return suites


def catalog_studies() -> list[tuple[str, bool, str]]:
    """(id, published, question)."""
    return [
        (sid, published, question)
        for sid, _n, published, question in list_study_summaries()
    ]


def catalog_scenarios() -> list[tuple[str, str, bool]]:
    """(id, suite, curated)."""
    return [
        (sid, suite, curated)
        for sid, suite, curated, _summary in list_scenario_summaries()
    ]


def default_run_output() -> str:
    """Directory for multi-cycle publish-set rounds (writes r1.json, r2.json, …)."""
    return str(ROOT / "perf" / "results" / "runs" / "publish-set-median")


def default_merge_output() -> str:
    return str(
        ROOT / "perf" / "results" / "runs" / "publish-set-median" / "median.json"
    )


def default_render_output() -> str:
    return str(ROOT / "perf" / "results" / "runs" / "tui-render.html")
