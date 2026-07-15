"""Generate operator-docs interop matrix markdown from results + cases.

Layout (publisher breakout):
  interop/index.md                 — hub: last tested, outcomes, summary, publisher links
  interop/conduit-behavior.md      — Conduit-behavior cases (single stub peer)
  interop/publishers/<slug>.md     — one page per publisher (A–Z); peer-matrix cases only
  interop/cases/<id>.md            — case intent
  interop/correctness-matrix.md    — short redirect stub for old links
"""

from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

from .cases import Case, load_cases
from .catalog import Peer, load_peers
from .paths import RESULTS_FILE, ROOT, load_json

OUT_DIR = ROOT / "operator-docs" / "docs" / "interop"
OUT_HUB = OUT_DIR / "index.md"
OUT_CONDUIT = OUT_DIR / "conduit-behavior.md"
OUT_MATRIX_STUB = OUT_DIR / "correctness-matrix.md"
OUT_PUBLISHERS = OUT_DIR / "publishers"
OUT_INTENTS = OUT_DIR / "cases"
MKDOCS_YML = ROOT / "operator-docs" / "mkdocs.yml"

# Representative peer for matrix: conduit cases (must match case applicability.peers).
CONDUIT_BEHAVIOR_PEER_ID = "thekelleys-dnsmasq-2.90"


def publisher_slug(publisher: str) -> str:
    slug = publisher.casefold()
    slug = re.sub(r"[^a-z0-9]+", "-", slug)
    return slug.strip("-") or "publisher"


def outcome_cell(outcome: str) -> str:
    """Markdown/HTML span for a matrix outcome; colors via interop-outcomes.css."""
    known = ("pass", "fail", "skip", "characterized")
    if outcome not in known:
        return outcome
    return (
        f'<span class="interop-outcome interop-outcome--{outcome}">'
        f"{outcome}</span>"
    )


def _group_peers_by_publisher(peers: list[Peer]) -> list[tuple[str, list[Peer]]]:
    """Return [(publisher, peers)] in publisher A–Z order; peers already sorted."""
    groups: dict[str, list[Peer]] = defaultdict(list)
    order: list[str] = []
    for peer in peers:
        if peer.publisher not in groups:
            order.append(peer.publisher)
        groups[peer.publisher].append(peer)
    return [(name, groups[name]) for name in order]


def _group_by_product(peers: list[Peer]) -> list[tuple[str, list[Peer]]]:
    products: dict[str, list[Peer]] = defaultdict(list)
    order: list[str] = []
    for peer in peers:
        if peer.product not in products:
            order.append(peer.product)
        products[peer.product].append(peer)
    return [(name, products[name]) for name in order]


def _write_intent_pages(cases: dict[str, Case]) -> None:
    OUT_INTENTS.mkdir(parents=True, exist_ok=True)
    for case in cases.values():
        intent_path = OUT_INTENTS / f"{case.id}.md"
        matrix_label = (
            "conduit ([Conduit behavior](/interop/conduit-behavior.md))"
            if case.is_conduit_matrix
            else "peer (by publisher)"
        )
        intent_path.write_text(
            f"# {case.id}\n\n{case.intent}\n\n"
            f"**Matrix:** {matrix_label}\n\n"
            f"**Suites:** {', '.join(case.suites)}\n\n"
            f"**Oracles:** {', '.join(o.get('kind', '?') for o in case.oracles)}\n",
            encoding="utf-8",
        )


def _format_tested_at(generated_at: str) -> str:
    """Prefer calendar date for operators; keep raw value if not ISO-shaped."""
    raw = (generated_at or "").strip()
    if len(raw) >= 10 and raw[4] == "-" and raw[7] == "-":
        return raw[:10]
    return raw or "unknown"


def executed_status_phrase(cells: list[dict]) -> str:
    """Summarize non-skip outcomes for a page-scoped cell set."""
    executed = [c for c in cells if c.get("outcome") not in (None, "skip")]
    if not executed:
        return "No executed cases (all out of scope)"
    fails = sum(1 for c in executed if c.get("outcome") == "fail")
    chars = sum(1 for c in executed if c.get("outcome") == "characterized")
    if fails:
        return f"Failures present ({fails} fail)"
    if chars:
        return f"No failures; {chars} characterized"
    return "All executed cases passed"


def profile_block_all_skips(outcomes: list[str | None]) -> bool:
    """True when a profile table has cells and every cell outcome is skip.

    Missing cells (None) prevent collapse so gaps stay visible as tables.
    """
    if not outcomes:
        return False
    return all(o == "skip" for o in outcomes)


def _last_tested_line(generated_at: str, status: str | None = None) -> list[str]:
    """Operator-facing provenance: date (+ optional executed-status summary)."""
    date = _format_tested_at(generated_at)
    if status:
        line = f"*Last tested {date} · {status}*"
    else:
        line = f"*Last tested {date}*"
    return [line, ""]


def _outcomes_legend() -> list[str]:
    return [
        "## Outcomes",
        "",
        "| Outcome | Meaning |",
        "|---------|---------|",
        f"| {outcome_cell('pass')} | Case checks met for this peer/version — the declared contract holds |",
        f"| {outcome_cell('fail')} | Unexpected mismatch or error — investigate Conduit forwarding or the peer path for this cell |",
        f"| {outcome_cell('skip')} | Out of scope for this peer role or profile (not a failure) |",
        f"| {outcome_cell('characterized')} | Documented peer-specific behavior (see the case page), not treated as a Conduit regression |",
        "",
        "Each [case page](/interop/cases/basic-a-forward.md) explains purpose, how the "
        "test runs, and what these outcomes mean for that case.",
        "",
    ]


def _running_locally() -> list[str]:
    """Operator-facing how-to; mirrors root Makefile `interop-*` targets."""
    return [
        "## Running these tests locally",
        "",
        "The matrices on this site are from a committed lab run. You can reproduce "
        "or explore the same harness on a machine with **Docker**, **Docker Compose**, "
        "and **Python 3** (with PyYAML). GitHub Actions does **not** execute the Docker "
        "suite; CI only checks that committed results stay fresh when harness inputs change.",
        "",
        "From a checkout of the DNSConduit repository:",
        "",
        "1. **Build a Conduit image** used as the system under test:",
        "",
        "    ```zsh",
        "    make interop-image",
        "    ```",
        "",
        "    This builds `conduit:local` via the repo `Dockerfile`. Override with "
        "`CONDUIT_IMAGE=…` if you already have an image tag.",
        "",
        "2. **Run the smoke suite** (all peers the smoke cases apply to). Peer images "
        "are pulled as needed; the first run can take a while:",
        "",
        "    ```zsh",
        "    make interop-smoke",
        "    ```",
        "",
        "3. **Optional — authoritative fixture case** (auth peers only):",
        "",
        "    ```zsh",
        "    make interop-auth",
        "    ```",
        "",
        "Those targets **print** pass/fail/skip lines; they do **not** rewrite "
        "`interop/results/latest.json` or regenerate this site. Named [cases](/interop/cases/basic-a-forward.md) "
        "document purpose, how each test works, and outcome implications.",
        "",
        "Useful extras:",
        "",
        "| Command | What it does |",
        "|---------|--------------|",
        "| `make interop-unit` | Fast harness unit tests (no Docker cells) |",
        "| `make interop-docs` | Rebuild these matrix pages from the committed `latest.json` |",
        "| `make interop-refresh` | Rebuild image, re-run smoke + auth, **write** results and regenerate docs (maintainers) |",
        "",
        "Filters (peer, case, profile) and pack layout: see `interop/README.md` in the "
        "repository. Override the image for any run target with "
        "`make interop-smoke CONDUIT_IMAGE=registry.example/conduit:1.2.3`.",
        "",
    ]


def _write_publisher_page(
    *,
    publisher: str,
    peers: list[Peer],
    cases: dict[str, Case],
    case_ids: list[str],
    profiles: list[str],
    index: dict[tuple[str, str, str], dict],
    generated_at: str,
) -> Path:
    slug = publisher_slug(publisher)
    path = OUT_PUBLISHERS / f"{slug}.md"
    page_cells: list[dict] = []
    for case_id in case_ids:
        for peer in peers:
            for profile in profiles:
                cell = index.get((case_id, peer.id, profile))
                if cell is not None:
                    page_cells.append(cell)
    status = executed_status_phrase(page_cells)

    lines: list[str] = [
        f"# {publisher}",
        "",
        f"{publisher} products under test for DNSConduit correctness. "
        "No peer is preferred or recommended. See the "
        "[interop overview](/interop/index.md).",
        "",
    ]
    lines.extend(_last_tested_line(generated_at, status))

    for product, product_peers in _group_by_product(peers):
        lines.append(f"## {product}")
        lines.append("")
        roles = sorted({p.role for p in product_peers})
        lines.append(f"**Role:** {', '.join(roles)}")
        lines.append("")

        out_of_scope_profiles: list[str] = []
        for profile in profiles:
            outcomes: list[str | None] = []
            for case_id in case_ids:
                for peer in product_peers:
                    cell = index.get((case_id, peer.id, profile))
                    outcomes.append(None if cell is None else cell.get("outcome"))
            if profile_block_all_skips(outcomes):
                out_of_scope_profiles.append(profile)
                continue

            lines.append(f"### Profile: `{profile}`")
            lines.append("")
            header = ["Test"] + [p.version for p in product_peers]
            lines.append("| " + " | ".join(header) + " |")
            lines.append("| " + " | ".join(["---"] * len(header)) + " |")
            for case_id in case_ids:
                case = cases.get(case_id)
                label = (
                    f"[`{case_id}`](/interop/cases/{case_id}.md)"
                    if case
                    else f"`{case_id}`"
                )
                row = [label]
                for peer in product_peers:
                    cell = index.get((case_id, peer.id, profile))
                    if cell is None:
                        row.append("—")
                    else:
                        row.append(outcome_cell(cell["outcome"]))
                lines.append("| " + " | ".join(row) + " |")
            lines.append("")

        if out_of_scope_profiles:
            listed = ", ".join(f"`{p}`" for p in out_of_scope_profiles)
            lines.append(
                f"Profiles with no in-scope peer-contract cases for this product: "
                f"{listed} (out of scope — not failures)."
            )
            lines.append("")

        lines.append("| Version | Peer id | Image |")
        lines.append("|---------|---------|-------|")
        for peer in product_peers:
            lines.append(f"| {peer.version} | `{peer.id}` | `{peer.image}` |")
        lines.append("")

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def _write_conduit_behavior_page(
    *,
    cases: dict[str, Case],
    conduit_case_ids: list[str],
    profiles: list[str],
    index: dict[tuple[str, str, str], dict],
    peers_by_id: dict[str, Peer],
    generated_at: str,
) -> Path:
    """Case × profile table for Conduit-behavior cases (single stub peer)."""
    peer = peers_by_id.get(CONDUIT_BEHAVIOR_PEER_ID)
    peer_label = (
        f"`{CONDUIT_BEHAVIOR_PEER_ID}` ({peer.product} {peer.version})"
        if peer
        else f"`{CONDUIT_BEHAVIOR_PEER_ID}`"
    )
    lines: list[str] = [
        "# Conduit behavior",
        "",
        "These cases exercise **Conduit** (lookup/cache path, request rules, dataplane "
        "runtime) rather than peer-product interoperability. They run against a "
        f"**single stub peer** ({peer_label}) so results are not spread across every "
        "publisher column. Peer contract cases remain under "
        "[By publisher](/interop/publishers/thekelleys.md).",
        "",
    ]
    conduit_cells: list[dict] = []
    for case_id in conduit_case_ids:
        for profile in profiles:
            cell = index.get((case_id, CONDUIT_BEHAVIOR_PEER_ID, profile))
            if cell is not None:
                conduit_cells.append(cell)
    lines.extend(
        _last_tested_line(generated_at, executed_status_phrase(conduit_cells))
    )

    # Profiles that appear for at least one conduit cell (or case applicability).
    profile_cols: list[str] = []
    for profile in profiles:
        if any(index.get((cid, CONDUIT_BEHAVIOR_PEER_ID, profile)) for cid in conduit_case_ids):
            profile_cols.append(profile)
    if not profile_cols:
        profile_cols = sorted(
            {
                p
                for cid in conduit_case_ids
                for p in (cases[cid].applicability.get("profiles") or [])
            }
        ) or ["forward-only"]

    lines.append("## Results")
    lines.append("")
    header = ["Test"] + [f"`{p}`" for p in profile_cols]
    lines.append("| " + " | ".join(header) + " |")
    lines.append("| " + " | ".join(["---"] * len(header)) + " |")
    for case_id in conduit_case_ids:
        label = f"[`{case_id}`](/interop/cases/{case_id}.md)"
        row = [label]
        for profile in profile_cols:
            cell = index.get((case_id, CONDUIT_BEHAVIOR_PEER_ID, profile))
            if cell is None:
                row.append("—")
            else:
                row.append(outcome_cell(cell["outcome"]))
        lines.append("| " + " | ".join(row) + " |")
    lines.append("")
    lines.append(
        f"Stub peer id: `{CONDUIT_BEHAVIOR_PEER_ID}`. "
        "Cases declare `matrix: conduit` and pin this peer in `applicability.peers`."
    )
    lines.append("")

    OUT_CONDUIT.parent.mkdir(parents=True, exist_ok=True)
    OUT_CONDUIT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return OUT_CONDUIT


def _summary_lines(
    cells: list[dict],
    peers_by_id: dict[str, Peer],
) -> list[str]:
    """Highlight non-pass outcomes for the hub."""
    interesting = [c for c in cells if c.get("outcome") in ("fail", "characterized")]
    lines = [
        "## Summary",
        "",
    ]
    if not interesting:
        lines.append(
            "No `fail` or `characterized` cells in the committed results. "
            "Open a publisher page for the full matrix (including `pass` and `skip`)."
        )
        lines.append("")
        return lines

    lines.append("| Outcome | Test | Publisher | Product | Version | Profile |")
    lines.append("|---------|------|-----------|---------|---------|---------|")
    rows: list[str] = []
    for cell in sorted(
        interesting,
        key=lambda c: (
            c.get("outcome", ""),
            c.get("case_id", ""),
            c.get("peer_id", ""),
        ),
    ):
        peer = peers_by_id.get(cell["peer_id"])
        if peer is None:
            # Drop cells for peers removed from the catalog (version refresh).
            continue
        pub = peer.publisher
        product = peer.product
        version = peer.version
        link = f"[{pub}](/interop/publishers/{publisher_slug(pub)}.md)"
        rows.append(
            "| "
            + " | ".join(
                [
                    outcome_cell(cell["outcome"]),
                    f"[`{cell['case_id']}`](/interop/cases/{cell['case_id']}.md)",
                    link,
                    product,
                    version,
                    f"`{cell.get('profile_id', '')}`",
                ]
            )
            + " |"
        )
    if not rows:
        # Only header remains — no current-catalog fails/characterized.
        return [
            "## Summary",
            "",
            "No `fail` or `characterized` cells for peers currently in the catalog. "
            "Open a publisher page for the full matrix (including `pass` and `skip`).",
            "",
        ]
    lines.extend(rows)
    lines.append("")
    return lines


def _update_mkdocs_nav(publisher_names: list[str], case_ids: list[str]) -> None:
    """Replace the Interop nav block with hub, Conduit behavior, Cases, publishers."""
    text = MKDOCS_YML.read_text(encoding="utf-8")
    start = text.find("  - Interop:\n")
    if start < 0:
        raise RuntimeError("Interop nav block not found in mkdocs.yml")
    # Find next top-level nav item after Interop (two spaces, dash, space, not more indented)
    rest = text[start + len("  - Interop:\n") :]
    end_rel = 0
    for match in re.finditer(r"\n  - [A-Za-z]", rest):
        end_rel = match.start()
        break
    if end_rel == 0:
        raise RuntimeError("Could not find end of Interop nav block")

    case_lines = ["      - Cases:"]
    for case_id in sorted(case_ids):
        case_lines.append(f"          - {case_id}: interop/cases/{case_id}.md")

    pub_lines = ["      - By publisher:"]
    for name in publisher_names:
        slug = publisher_slug(name)
        pub_lines.append(f"          - {name}: interop/publishers/{slug}.md")

    block = (
        "  - Interop:\n"
        "      - Overview: interop/index.md\n"
        "      - Conduit behavior: interop/conduit-behavior.md\n"
        + "\n".join(case_lines)
        + "\n"
        + "\n".join(pub_lines)
        + "\n"
    )
    # rest[end_rel:] starts with "\n  - <next top-level nav item>"
    new_text = text[:start] + block.rstrip("\n") + rest[end_rel:]
    MKDOCS_YML.write_text(new_text, encoding="utf-8")


def generate_matrix(
    *,
    results_path: Path = RESULTS_FILE,
    out_page: Path | None = None,
) -> Path:
    """Generate hub + per-publisher pages. Returns hub path (out_page kept for CLI compat)."""
    peers = load_peers()
    cases = {c.id: c for c in load_cases()}
    if not results_path.is_file():
        raise FileNotFoundError(f"missing results: {results_path}")
    results = load_json(results_path)
    generated_at = results.get("generated_at", "unknown")
    cells: list[dict] = list(results.get("cells", []))

    index: dict[tuple[str, str, str], dict] = {}
    for cell in cells:
        key = (cell["case_id"], cell["peer_id"], cell["profile_id"])
        index[key] = cell

    profiles = sorted({c["profile_id"] for c in cells}) or ["forward-only"]
    case_ids = sorted({c["case_id"] for c in cells} | set(cases))
    peer_case_ids = sorted(
        cid for cid in case_ids if cid in cases and not cases[cid].is_conduit_matrix
    )
    conduit_case_ids = sorted(cid for cid, case in cases.items() if case.is_conduit_matrix)
    peers_by_id = {p.id: p for p in peers}
    publisher_groups = _group_peers_by_publisher(peers)

    _write_intent_pages(cases)

    # Wipe stale publisher pages then rewrite
    OUT_PUBLISHERS.mkdir(parents=True, exist_ok=True)
    for old in OUT_PUBLISHERS.glob("*.md"):
        old.unlink()

    publisher_names: list[str] = []
    for publisher, pub_peers in publisher_groups:
        publisher_names.append(publisher)
        _write_publisher_page(
            publisher=publisher,
            peers=pub_peers,
            cases=cases,
            case_ids=peer_case_ids,
            profiles=profiles,
            index=index,
            generated_at=generated_at,
        )

    _write_conduit_behavior_page(
        cases=cases,
        conduit_case_ids=conduit_case_ids,
        profiles=profiles,
        index=index,
        peers_by_id=peers_by_id,
        generated_at=generated_at,
    )

    hub: list[str] = [
        "# Interop",
        "",
        "Published correctness results for DNSConduit against peer DNS software "
        "under test. **Peer contract** cases are split **by publisher** (alphabetical). "
        "**Conduit behavior** cases (cache path, rules, dataplane runtime) use a single "
        "stub peer — see [Conduit behavior](/interop/conduit-behavior.md). "
        "No peer is preferred or recommended.",
        "",
        "By default, Conduit’s forward path **passes peer response shapes through** "
        "(rcode, answer section, and flags such as AA/TC) so operators see the same "
        "backend quirks they would when querying the peer directly. Cases that "
        "document those quirks use parity against a direct dig; "
        "`characterized` cells record expected peer-specific shapes. Configuration "
        "that rewrites or sanitizes peer answers is covered separately when those "
        "knobs are under test.",
        "",
        "## Matrices",
        "",
        "- [Conduit behavior](/interop/conduit-behavior.md) — Conduit-focused cases (stub peer)",
        "",
        "## Publishers",
        "",
    ]
    for publisher in publisher_names:
        slug = publisher_slug(publisher)
        products = ", ".join(
            dict.fromkeys(p.product for p in peers if p.publisher == publisher)
        )
        hub.append(f"- [{publisher}](/interop/publishers/{slug}.md) — {products}")
    hub.append("")
    hub.extend(_last_tested_line(generated_at, executed_status_phrase(cells)))
    hub.extend(_outcomes_legend())
    hub.extend(_running_locally())
    hub.extend(_summary_lines(cells, peers_by_id))

    hub.extend(
        [
            "## Peer catalog",
            "",
            "Peers are software under test. Ordering is publisher (A–Z), product (A–Z), "
            "version ascending.",
            "",
            "| Publisher | Product | Version | Role | Id |",
            "|-----------|---------|---------|------|----|",
        ]
    )
    for peer in peers:
        slug = publisher_slug(peer.publisher)
        hub.append(
            f"| [{peer.publisher}](/interop/publishers/{slug}.md) | {peer.product} | "
            f"{peer.version} | {peer.role} | `{peer.id}` |"
        )
    hub.append("")

    OUT_HUB.write_text("\n".join(hub) + "\n", encoding="utf-8")

    stub = (
        "# Correctness matrix\n\n"
        "The interop matrix is split **by publisher**, plus a "
        "[Conduit behavior](/interop/conduit-behavior.md) page for Conduit-focused cases. "
        "Start at the [interop overview](/interop/index.md).\n"
    )
    OUT_MATRIX_STUB.write_text(stub, encoding="utf-8")

    _update_mkdocs_nav(publisher_names, list(cases.keys()))

    return out_page or OUT_HUB
