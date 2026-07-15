"""Case catalog loader."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .paths import CASES, load_yaml


@dataclass
class Case:
    id: str
    intent: str
    suites: list[str]
    applicability: dict[str, Any]
    oracles: list[dict[str, Any]]
    path: Path = field(repr=False)
    peer_setup: dict[str, Any] = field(default_factory=dict)
    conduit_delta: dict[str, Any] = field(default_factory=dict)
    # Optional per-step queries; empty → single dig using CLI/default or fixture qname.
    # Per step may include sleep_before_secs (float/int) before that dig.
    queries: list[dict[str, Any]] = field(default_factory=list)
    # Files copied into the cell's /etc/conduit/assets/ (src relative to interop/).
    conduit_assets: list[dict[str, str]] = field(default_factory=list)
    # "peer" (default): publisher matrices. "conduit": Conduit-behavior page; one stub peer.
    matrix: str = "peer"

    def applies_to(self, *, role: str, profile_id: str, peer_id: str) -> bool:
        roles = self.applicability.get("roles")
        if roles and role not in roles:
            return False
        profiles = self.applicability.get("profiles")
        if profiles and profile_id not in profiles:
            return False
        peers = self.applicability.get("peers")
        if peers and peer_id not in peers:
            return False
        return True

    @property
    def is_conduit_matrix(self) -> bool:
        return self.matrix == "conduit"


def load_cases(directory: Path = CASES) -> list[Case]:
    cases: list[Case] = []
    for path in sorted(directory.glob("*.yaml")):
        raw = load_yaml(path)
        matrix = str(raw.get("matrix") or "peer").strip().lower()
        if matrix not in ("peer", "conduit"):
            raise ValueError(f"{path}: matrix must be 'peer' or 'conduit', got {matrix!r}")
        cases.append(
            Case(
                id=raw["id"],
                intent=raw.get("intent", "").strip(),
                suites=list(raw.get("suites", ["full"])),
                applicability=dict(raw.get("applicability", {})),
                oracles=list(raw.get("oracles", [])),
                peer_setup=dict(raw.get("peer_setup") or {}),
                conduit_delta=dict(raw.get("conduit_delta") or {}),
                queries=list(raw.get("queries") or []),
                conduit_assets=[
                    {"src": str(a["src"]), "dest": str(a["dest"])}
                    for a in (raw.get("conduit_assets") or [])
                ],
                matrix=matrix,
                path=path,
            )
        )
    return cases


def case_by_id(case_id: str) -> Case:
    for case in load_cases():
        if case.id == case_id:
            return case
    raise KeyError(f"unknown case id: {case_id}")
