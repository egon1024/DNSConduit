"""Oracle evaluation helpers (parity / fixture / property / differential)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from .paths import INTEROP, load_json


@dataclass
class QueryResult:
    rcode: str
    ancount: int
    answers: list[dict[str, Any]]
    raw: str = ""


def evaluate_oracles(
    oracles: list[dict[str, Any]],
    *,
    via_conduit: QueryResult | None,
    direct: QueryResult | None,
    peer_id: str,
) -> tuple[str, str]:
    """
    Return (outcome, detail). Outcomes: pass | fail | characterized.
    Caller handles skip before invoking.
    """
    if via_conduit is None:
        return "fail", "no response via Conduit"

    details: list[str] = []
    for oracle in oracles:
        kind = oracle.get("kind")
        if kind == "property":
            ok, msg = _property(via_conduit, oracle.get("checks", []))
            if not ok:
                return "fail", msg
            details.append(msg)
        elif kind == "parity":
            if direct is None:
                return "fail", "parity oracle requires direct peer baseline"
            ok, msg = _parity(via_conduit, direct, oracle.get("compare", ["rcode", "ancount"]))
            if not ok:
                return "fail", msg
            details.append(msg)
        elif kind == "fixture":
            ok, msg = _fixture(via_conduit, oracle["path"])
            if not ok:
                return "fail", msg
            details.append(msg)
        elif kind == "differential":
            expected = oracle.get("expect", {})
            peer_expect = expected.get(peer_id) or expected.get("default")
            if peer_expect is None:
                return "characterized", f"no differential expectation for {peer_id}"
            ok, msg = _match_expect(via_conduit, peer_expect)
            if ok:
                return "characterized", msg
            return "fail", msg
        else:
            return "fail", f"unknown oracle kind: {kind}"
    return "pass", "; ".join(details) if details else "ok"


def _property(result: QueryResult, checks: list[str]) -> tuple[bool, str]:
    for check in checks:
        if check == "rcode-noerror":
            if result.rcode.upper() != "NOERROR":
                return False, f"expected NOERROR got {result.rcode}"
        elif check == "has-answer":
            if result.ancount < 1 and not result.answers:
                return False, "expected at least one answer"
        else:
            return False, f"unknown property check: {check}"
    return True, "property ok"


def _parity(via: QueryResult, direct: QueryResult, fields: list[str]) -> tuple[bool, str]:
    for field in fields:
        if field == "rcode" and via.rcode.upper() != direct.rcode.upper():
            return False, f"rcode mismatch conduit={via.rcode} direct={direct.rcode}"
        if field == "ancount" and via.ancount != direct.ancount:
            return False, f"ancount mismatch conduit={via.ancount} direct={direct.ancount}"
    return True, "parity ok"


def _fixture(result: QueryResult, rel_path: str) -> tuple[bool, str]:
    path = INTEROP / rel_path if not rel_path.startswith("/") else INTEROP.parent / rel_path
    if not path.is_file():
        # Allow paths relative to interop/
        alt = INTEROP / rel_path.removeprefix("fixtures/")
        path = INTEROP / "fixtures" / rel_path.removeprefix("fixtures/") if "fixtures" in rel_path else path
    # cases use path: fixtures/zones/...
    path = INTEROP / rel_path
    if not path.is_file():
        return False, f"fixture missing: {rel_path}"
    expected = load_json(path)
    if result.rcode.upper() != str(expected.get("rcode", "NOERROR")).upper():
        return False, f"fixture rcode want {expected.get('rcode')} got {result.rcode}"
    want = {(a["name"].rstrip("."), a["type"], a["rdata"]) for a in expected.get("answers", [])}
    got = {
        (a.get("name", "").rstrip("."), a.get("type", ""), a.get("rdata", ""))
        for a in result.answers
    }
    if want and not want.issubset(got):
        return False, f"fixture answers missing: want {want} got {got}"
    return True, "fixture ok"


def _match_expect(result: QueryResult, expect: dict[str, Any]) -> tuple[bool, str]:
    if "rcode" in expect and result.rcode.upper() != str(expect["rcode"]).upper():
        return False, f"differential rcode want {expect['rcode']} got {result.rcode}"
    return True, "differential match"
