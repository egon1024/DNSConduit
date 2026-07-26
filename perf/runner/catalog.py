"""Scenario and annotation catalog loading."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .paths import ANNOTATIONS, SCENARIOS, load_yaml

VALID_SUITES = frozenset(
    {
        "micro",
        "scale",
        "feature_tax",
        "lifecycle",
        "shutdown_drain",
        "lossless_upgrade",
    }
)


@dataclass(frozen=True)
class Scenario:
    id: str
    suite: str
    intent: str
    axes: dict[str, Any]
    recipe: dict[str, Any]
    curated: bool = False
    path: Path | None = None

    @classmethod
    def from_dict(cls, raw: dict[str, Any], path: Path | None = None) -> Scenario:
        if raw.get("schema_version") != 1:
            raise ValueError(f"unsupported scenario schema_version in {path}")
        sid = raw["id"]
        suite = raw["suite"]
        if suite not in VALID_SUITES:
            raise ValueError(f"scenario {sid}: unknown suite {suite!r}")
        return cls(
            id=sid,
            suite=suite,
            intent=raw.get("intent", "").strip(),
            axes=dict(raw.get("axes") or {}),
            recipe=dict(raw.get("recipe") or {}),
            curated=bool(raw.get("curated", False)),
            path=path,
        )


@dataclass(frozen=True)
class Annotation:
    id: str
    tone: str
    title: str
    body: str
    related_scenarios: tuple[str, ...] = ()
    related_releases: tuple[str, ...] = ()
    path: Path | None = None

    VALID_TONES = frozenset(
        {"improved_because", "regressed_because", "context", "known_noise"}
    )

    @classmethod
    def from_dict(cls, raw: dict[str, Any], path: Path | None = None) -> Annotation:
        if raw.get("schema_version") != 1:
            raise ValueError(f"unsupported annotation schema_version in {path}")
        tone = raw["tone"]
        if tone not in cls.VALID_TONES:
            raise ValueError(f"annotation {raw.get('id')}: invalid tone {tone!r}")
        return cls(
            id=raw["id"],
            tone=tone,
            title=raw["title"],
            body=raw["body"],
            related_scenarios=tuple(raw.get("related_scenarios") or ()),
            related_releases=tuple(raw.get("related_releases") or ()),
            path=path,
        )


def load_scenarios(directory: Path = SCENARIOS) -> list[Scenario]:
    scenarios: list[Scenario] = []
    if not directory.is_dir():
        return scenarios
    for path in sorted(directory.glob("*.yaml")):
        if path.name == "schema.yaml":
            continue
        raw = load_yaml(path)
        if not isinstance(raw, dict):
            raise ValueError(f"scenario file must be a mapping: {path}")
        scenarios.append(Scenario.from_dict(raw, path=path))
    return scenarios


def filter_scenarios(
    scenarios: list[Scenario],
    *,
    suite: str | None = None,
    scenario_id: str | None = None,
) -> list[Scenario]:
    out = scenarios
    if suite:
        out = [s for s in out if s.suite == suite]
    if scenario_id:
        out = [s for s in out if s.id == scenario_id]
    return out


def load_annotations(directory: Path = ANNOTATIONS) -> list[Annotation]:
    anns: list[Annotation] = []
    if not directory.is_dir():
        return anns
    for path in sorted(directory.glob("*.yaml")):
        if path.name == "schema.yaml":
            continue
        raw = load_yaml(path)
        if not isinstance(raw, dict):
            raise ValueError(f"annotation file must be a mapping: {path}")
        anns.append(Annotation.from_dict(raw, path=path))
    return anns
