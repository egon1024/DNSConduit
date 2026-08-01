"""Scenario, study, and annotation catalog loading."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .paths import ANNOTATIONS, SCENARIOS, STUDIES, load_yaml

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
    suites: list[str] | None = None,
    scenario_id: str | None = None,
    scenario_ids: list[str] | None = None,
    curated_only: bool = False,
) -> list[Scenario]:
    out = scenarios
    suite_set: set[str] = set()
    if suite:
        suite_set.add(suite)
    if suites:
        suite_set.update(suites)
    if suite_set:
        out = [s for s in out if s.suite in suite_set]
    id_set: set[str] = set()
    if scenario_id:
        id_set.add(scenario_id)
    if scenario_ids:
        id_set.update(scenario_ids)
    if id_set:
        out = [s for s in out if s.id in id_set]
    if curated_only:
        out = [s for s in out if s.curated]
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


@dataclass(frozen=True)
class StudyFigure:
    id: str
    title: str
    members: tuple[str, ...]
    y_label: str = "Achieved QPS"
    category_axis: str | None = None


@dataclass(frozen=True)
class Study:
    id: str
    title: str
    question: str
    published: bool
    members: tuple[str, ...]
    compare_axis: str
    primary_metric: str
    figures: tuple[StudyFigure, ...]
    takeaway: str = ""
    related_guides: tuple[str, ...] = ()
    annotation_ids: tuple[str, ...] = ()
    path: Path | None = None

    @classmethod
    def from_dict(cls, raw: dict[str, Any], path: Path | None = None) -> Study:
        if raw.get("schema_version") != 1:
            raise ValueError(f"unsupported study schema_version in {path}")
        sid = raw["id"]
        members = tuple(raw.get("members") or ())
        if not members:
            raise ValueError(f"study {sid}: members must be non-empty")
        figures_raw = raw.get("figures") or []
        if not figures_raw:
            raise ValueError(f"study {sid}: figures must be non-empty")
        figures: list[StudyFigure] = []
        member_set = set(members)
        for fig in figures_raw:
            fig_members = tuple(fig.get("members") or ())
            if not fig_members:
                raise ValueError(f"study {sid}: figure {fig.get('id')!r} needs members")
            unknown = [m for m in fig_members if m not in member_set]
            if unknown:
                raise ValueError(
                    f"study {sid}: figure {fig.get('id')!r} references "
                    f"non-member scenario ids: {unknown}"
                )
            figures.append(
                StudyFigure(
                    id=fig["id"],
                    title=fig["title"],
                    members=fig_members,
                    y_label=str(fig.get("y_label") or "Achieved QPS"),
                    category_axis=fig.get("category_axis"),
                )
            )
        return cls(
            id=sid,
            title=raw["title"],
            question=str(raw.get("question") or "").strip(),
            published=bool(raw.get("published", False)),
            members=members,
            compare_axis=str(raw.get("compare_axis") or ""),
            primary_metric=str(raw.get("primary_metric") or "achieved_qps"),
            figures=tuple(figures),
            takeaway=str(raw.get("takeaway") or "").strip(),
            related_guides=tuple(raw.get("related_guides") or ()),
            annotation_ids=tuple(raw.get("annotation_ids") or ()),
            path=path,
        )


def load_studies(
    directory: Path = STUDIES,
    *,
    scenarios: list[Scenario] | None = None,
) -> list[Study]:
    """Load studies; optionally validate member ids against a scenario list."""
    studies: list[Study] = []
    if not directory.is_dir():
        return studies
    seen: set[str] = set()
    scenario_ids = {s.id for s in scenarios} if scenarios is not None else None
    for path in sorted(directory.glob("*.yaml")):
        if path.name == "schema.yaml":
            continue
        raw = load_yaml(path)
        if not isinstance(raw, dict):
            raise ValueError(f"study file must be a mapping: {path}")
        study = Study.from_dict(raw, path=path)
        if study.id in seen:
            raise ValueError(f"duplicate study id: {study.id}")
        seen.add(study.id)
        if scenario_ids is not None:
            missing = [m for m in study.members if m not in scenario_ids]
            if missing:
                raise ValueError(
                    f"study {study.id}: unknown member scenario ids: {missing}"
                )
        studies.append(study)
    return studies


def get_study(studies: list[Study], study_id: str) -> Study:
    for study in studies:
        if study.id == study_id:
            return study
    raise KeyError(study_id)


def publish_set_member_ids(studies: list[Study]) -> list[str]:
    """Ordered union of members from studies marked published."""
    out: list[str] = []
    seen: set[str] = set()
    for study in studies:
        if not study.published:
            continue
        for mid in study.members:
            if mid not in seen:
                seen.add(mid)
                out.append(mid)
    return out


def resolve_scenario_ids_from_studies(
    studies: list[Study],
    *,
    study_ids: list[str] | None = None,
    publish_set: bool = False,
) -> list[str]:
    """Expand study filters to ordered, deduplicated scenario ids."""
    if not study_ids and not publish_set:
        return []
    by_id = {s.id: s for s in studies}
    ordered: list[str] = []
    seen: set[str] = set()

    def _extend(members: tuple[str, ...] | list[str]) -> None:
        for mid in members:
            if mid not in seen:
                seen.add(mid)
                ordered.append(mid)

    if publish_set:
        _extend(publish_set_member_ids(studies))
    for sid in study_ids or []:
        if sid not in by_id:
            raise KeyError(sid)
        _extend(by_id[sid].members)
    return ordered


def select_scenarios(
    scenarios: list[Scenario],
    *,
    suite: str | None = None,
    suites: list[str] | None = None,
    scenario_id: str | None = None,
    scenario_ids: list[str] | None = None,
    curated_only: bool = False,
    study_ids: list[str] | None = None,
    publish_set: bool = False,
    studies: list[Study] | None = None,
) -> list[Scenario]:
    """Filter scenarios; study/publish-set selection preserves member order."""
    id_list: list[str] | None = None
    if study_ids or publish_set:
        catalog = studies if studies is not None else load_studies(scenarios=scenarios)
        try:
            id_list = resolve_scenario_ids_from_studies(
                catalog, study_ids=study_ids, publish_set=publish_set
            )
        except KeyError as exc:
            raise ValueError(f"unknown study id: {exc.args[0]}") from exc
        if scenario_id or scenario_ids:
            extra = list(scenario_ids or [])
            if scenario_id:
                extra.append(scenario_id)
            # Intersection: keep study order, drop ids not also requested.
            allow = set(extra)
            id_list = [i for i in id_list if i in allow]
    elif scenario_id or scenario_ids:
        id_list = []
        if scenario_ids:
            id_list.extend(scenario_ids)
        if scenario_id and scenario_id not in id_list:
            id_list.append(scenario_id)

    if id_list is not None:
        by_id = {s.id: s for s in scenarios}
        missing = [i for i in id_list if i not in by_id]
        if missing:
            raise ValueError(f"unknown scenario id(s): {missing}")
        ordered = [by_id[i] for i in id_list]
        return filter_scenarios(
            ordered,
            suite=suite,
            suites=suites,
            curated_only=curated_only,
        )

    return filter_scenarios(
        scenarios,
        suite=suite,
        suites=suites,
        scenario_id=scenario_id,
        scenario_ids=scenario_ids,
        curated_only=curated_only,
    )
