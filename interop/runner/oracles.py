"""Oracle evaluation helpers (parity / fixture / property / differential / sequence)."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .paths import INTEROP, load_json


@dataclass
class QueryResult:
    rcode: str
    ancount: int
    answers: list[dict[str, Any]]
    raw: str = ""
    flags: dict[str, bool] = field(default_factory=dict)
    nscount: int = 0
    arcount: int = 0
    edns_udp_size: int | None = None

    @property
    def answer_types(self) -> set[str]:
        return {str(a.get("type", "")).upper() for a in self.answers if a.get("type")}

    @property
    def has_cname(self) -> bool:
        return "CNAME" in self.answer_types


def evaluate_oracles(
    oracles: list[dict[str, Any]],
    *,
    via_conduit: QueryResult | None,
    direct: QueryResult | None,
    peer_id: str,
    via_steps: list[QueryResult] | None = None,
) -> tuple[str, str]:
    """
    Return (outcome, detail). Outcomes: pass | fail | characterized.
    Caller handles skip before invoking.

    Per-step oracles (property/parity/fixture/differential) run against each entry in
    ``via_steps`` when provided, otherwise against ``via_conduit`` once.
    Sequence oracles run once against the full step list.
    """
    if via_conduit is None and not via_steps:
        return "fail", "no response via Conduit"

    steps = via_steps if via_steps is not None else ([via_conduit] if via_conduit else [])
    if not steps or any(s is None for s in steps):
        return "fail", "no response via Conduit"

    details: list[str] = []
    for oracle in oracles:
        kind = oracle.get("kind")
        if kind == "sequence":
            ok, msg = _sequence(steps, oracle.get("checks", []))
            if not ok:
                return "fail", msg
            details.append(msg)
            continue

        for idx, via in enumerate(steps):
            assert via is not None
            if kind == "property":
                ok, msg = _property(via, oracle)
                if not ok:
                    return "fail", f"step {idx + 1}: {msg}"
                details.append(msg)
            elif kind == "parity":
                if direct is None:
                    return "fail", "parity oracle requires direct peer baseline"
                # Parity compares the single conduit dig to a concurrent direct dig.
                # When via_steps is used, parity still binds to the last/direct pair
                # supplied by the caller (legacy single-query cells).
                cmp_via = via if len(steps) == 1 else (via_conduit or via)
                ok, msg = _parity(cmp_via, direct, oracle.get("compare", ["rcode", "ancount"]))
                if not ok:
                    return "fail", msg
                details.append(msg)
                break
            elif kind == "fixture":
                ok, msg = _fixture(via, oracle["path"])
                if not ok:
                    return "fail", f"step {idx + 1}: {msg}"
                details.append(msg)
            elif kind == "differential":
                expected = oracle.get("expect", {})
                peer_expect = expected.get(peer_id) or expected.get("default")
                if peer_expect is None:
                    return "characterized", f"no differential expectation for {peer_id}"
                ok, msg = _match_expect(via, peer_expect)
                if ok:
                    return "characterized", msg
                return "fail", msg
            else:
                return "fail", f"unknown oracle kind: {kind}"
    return "pass", "; ".join(details) if details else "ok"


def _property(result: QueryResult, oracle: dict[str, Any]) -> tuple[bool, str]:
    checks = oracle.get("checks", [])
    for check in checks:
        if check == "rcode-noerror":
            if result.rcode.upper() != "NOERROR":
                return False, f"expected NOERROR got {result.rcode}"
        elif check == "rcode-nxdomain":
            if result.rcode.upper() != "NXDOMAIN":
                return False, f"expected NXDOMAIN got {result.rcode}"
        elif check == "rcode-refused":
            if result.rcode.upper() != "REFUSED":
                return False, f"expected REFUSED got {result.rcode}"
        elif check == "has-answer":
            if result.ancount < 1 and not result.answers:
                return False, "expected at least one answer"
        elif check == "empty-answer":
            # NODATA and similar: allow any rcode so long as answer section is empty.
            if result.answers or result.ancount > 0:
                return False, f"expected empty answer got ancount={result.ancount}"
        elif check == "nodata":
            if result.rcode.upper() != "NOERROR":
                return False, f"expected NODATA (NOERROR) got {result.rcode}"
            if result.answers or result.ancount > 0:
                return False, f"expected NODATA empty answer got ancount={result.ancount}"
        elif check == "no-answer":
            # Soft/hard drop: dig timeout or no usable answer (TIMEOUT/UNKNOWN/empty).
            rcode = result.rcode.upper()
            if result.answers or result.ancount > 0:
                return False, f"expected no answer RRs got ancount={result.ancount}"
            if rcode == "NOERROR":
                return False, "expected no successful answer; got NOERROR"
            if rcode not in ("TIMEOUT", "UNKNOWN", "SERVFAIL", "REFUSED", "FORMERR"):
                # Still accept empty answers for other rcodes without RRs.
                pass
        elif check == "answer-rdata-set":
            want = {str(x) for x in (oracle.get("answer_rdata") or [])}
            if not want:
                return False, "answer-rdata-set requires answer_rdata"
            got = {str(a.get("rdata", "")) for a in result.answers}
            if got != want:
                return False, f"answer rdata set want {sorted(want)} got {sorted(got)}"
        elif check == "answer-types":
            want = {str(x).upper() for x in (oracle.get("answer_types") or [])}
            if not want:
                return False, "answer-types requires answer_types"
            if result.answer_types != want:
                return False, (
                    f"answer types want {sorted(want)} got {sorted(result.answer_types)}"
                )
        elif check == "has-cname":
            if not result.has_cname:
                return False, "expected CNAME in answer section"
        elif check == "flag-aa":
            if not result.flags.get("aa"):
                return False, "expected aa flag set"
        elif check == "flag-tc":
            if not result.flags.get("tc"):
                return False, "expected tc flag set"
        elif check == "has-edns":
            if result.edns_udp_size is None:
                return False, "expected EDNS in response"
        else:
            return False, f"unknown property check: {check}"
    return True, "property ok"


def _answer_order_key(result: QueryResult) -> tuple[str, ...]:
    return tuple(str(a.get("rdata", "")) for a in result.answers)


def _sequence(steps: list[QueryResult], checks: list[str]) -> tuple[bool, str]:
    for check in checks:
        if check == "answer-order-varies":
            orders = {_answer_order_key(s) for s in steps if s.answers}
            if len(orders) < 2:
                return False, (
                    f"expected answer order to vary across queries; "
                    f"saw {len(orders)} distinct order(s)"
                )
        else:
            return False, f"unknown sequence check: {check}"
    return True, "sequence ok"


def _parity(via: QueryResult, direct: QueryResult, fields: list[str]) -> tuple[bool, str]:
    for field in fields:
        if field == "rcode" and via.rcode.upper() != direct.rcode.upper():
            return False, f"rcode mismatch conduit={via.rcode} direct={direct.rcode}"
        if field == "ancount" and via.ancount != direct.ancount:
            return False, f"ancount mismatch conduit={via.ancount} direct={direct.ancount}"
        if field == "answer-rdata-set":
            via_set = {str(a.get("rdata", "")) for a in via.answers}
            direct_set = {str(a.get("rdata", "")) for a in direct.answers}
            if via_set != direct_set:
                return False, (
                    f"answer-rdata-set mismatch conduit={sorted(via_set)} "
                    f"direct={sorted(direct_set)}"
                )
        if field == "answer-types":
            if via.answer_types != direct.answer_types:
                return False, (
                    f"answer-types mismatch conduit={sorted(via.answer_types)} "
                    f"direct={sorted(direct.answer_types)}"
                )
        if field == "aa":
            if bool(via.flags.get("aa")) != bool(direct.flags.get("aa")):
                return False, (
                    f"aa flag mismatch conduit={via.flags.get('aa')} "
                    f"direct={direct.flags.get('aa')}"
                )
        if field == "tc":
            if bool(via.flags.get("tc")) != bool(direct.flags.get("tc")):
                return False, (
                    f"tc flag mismatch conduit={via.flags.get('tc')} "
                    f"direct={direct.flags.get('tc')}"
                )
        if field == "has-cname":
            if via.has_cname != direct.has_cname:
                return False, (
                    f"has-cname mismatch conduit={via.has_cname} direct={direct.has_cname}"
                )
        if field == "edns":
            # Both present or both absent is enough for passthrough smoke.
            via_edns = via.edns_udp_size is not None
            direct_edns = direct.edns_udp_size is not None
            if via_edns != direct_edns:
                return False, f"edns presence mismatch conduit={via_edns} direct={direct_edns}"
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
    if "ancount" in expect and result.ancount != int(expect["ancount"]):
        return False, f"differential ancount want {expect['ancount']} got {result.ancount}"
    if "answer_types" in expect:
        want = {str(x).upper() for x in expect["answer_types"]}
        if result.answer_types != want:
            return False, (
                f"differential answer_types want {sorted(want)} "
                f"got {sorted(result.answer_types)}"
            )
    if "has_cname" in expect and bool(result.has_cname) != bool(expect["has_cname"]):
        return False, f"differential has_cname want {expect['has_cname']} got {result.has_cname}"
    if "flags" in expect:
        want_flags = expect["flags"] or {}
        for name, want_val in want_flags.items():
            got_val = bool(result.flags.get(str(name).lower()))
            if got_val != bool(want_val):
                return False, f"differential flag {name} want {want_val} got {got_val}"
    return True, "differential match"
