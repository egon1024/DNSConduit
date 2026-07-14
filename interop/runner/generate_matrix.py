"""Generate operator-docs interop matrix markdown from results + cases.

Layout (publisher breakout):
  interop/index.md                 — hub: provenance, outcomes, summary, publisher links
  interop/publishers/<slug>.md     — one page per publisher (A–Z)
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
OUT_MATRIX_STUB = OUT_DIR / "correctness-matrix.md"
OUT_PUBLISHERS = OUT_DIR / "publishers"
OUT_INTENTS = OUT_DIR / "cases"
MKDOCS_YML = ROOT / "operator-docs" / "mkdocs.yml"


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
        intent_path.write_text(
            f"# {case.id}\n\n{case.intent}\n\n"
            f"**Suites:** {', '.join(case.suites)}\n\n"
            f"**Oracles:** {', '.join(o.get('kind', '?') for o in case.oracles)}\n",
            encoding="utf-8",
        )


def _provenance_table(generated_at: str, provenance: dict, fp: str) -> list[str]:
    return [
        "## Last tested",
        "",
        "| Field | Value |",
        "|-------|-------|",
        f"| Generated at | `{generated_at}` |",
        f"| Conduit version | `{provenance.get('conduit_version', 'unknown')}` |",
        f"| Conduit image | `{provenance.get('conduit_image', 'unknown')}` |",
        # Lab build digest of the Conduit image used for this run — not a GitHub
        # Release asset digest. Useful to reproduce a lab blob; omit confusion by
        # labeling it explicitly.
        f"| Conduit image digest (lab) | `{provenance.get('conduit_image_digest', 'unknown')}` |",
        f"| Inputs fingerprint | `{fp}` |",
        "",
        "The **inputs fingerprint** is a sha256 over harness inputs "
        "(`interop/catalog`, fixtures, compose, runner, results schema). "
        "CI uses it to detect when committed matrix results are stale relative "
        "to those inputs. It is not a product version.",
        "",
    ]


def _outcomes_legend() -> list[str]:
    return [
        "## Outcomes",
        "",
        "| Outcome | Meaning |",
        "|---------|---------|",
        f"| {outcome_cell('pass')} | Declared oracles succeeded |",
        f"| {outcome_cell('fail')} | Unexpected mismatch or error |",
        f"| {outcome_cell('skip')} | Case not applicable to this peer/profile |",
        f"| {outcome_cell('characterized')} | Expected peer-specific behavior (see case intent) |",
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
    provenance: dict,
    fp: str,
) -> Path:
    slug = publisher_slug(publisher)
    path = OUT_PUBLISHERS / f"{slug}.md"
    lines: list[str] = [
        f"# {publisher}",
        "",
        f"{publisher} products under test for DNSConduit correctness. "
        "No peer is preferred or recommended. See the "
        "[interop overview](/interop/index.md) for provenance shared across publishers.",
        "",
    ]
    lines.extend(_provenance_table(generated_at, provenance, fp))

    for product, product_peers in _group_by_product(peers):
        lines.append(f"## {product}")
        lines.append("")
        roles = sorted({p.role for p in product_peers})
        lines.append(f"**Role:** {', '.join(roles)}")
        lines.append("")

        for profile in profiles:
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

        lines.append("| Version | Peer id | Image |")
        lines.append("|---------|---------|-------|")
        for peer in product_peers:
            lines.append(f"| {peer.version} | `{peer.id}` | `{peer.image}` |")
        lines.append("")

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


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


def _update_mkdocs_nav(publisher_names: list[str]) -> None:
    """Replace the Interop nav block with hub + alphabetical publisher pages."""
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

    pub_lines = ["      - By publisher:"]
    for name in publisher_names:
        slug = publisher_slug(name)
        pub_lines.append(f"          - {name}: interop/publishers/{slug}.md")

    block = (
        "  - Interop:\n"
        "      - Overview: interop/index.md\n"
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
    provenance = results.get("provenance", {})
    generated_at = results.get("generated_at", "unknown")
    fp = results.get("inputs_fingerprint", "unknown")
    cells: list[dict] = list(results.get("cells", []))

    index: dict[tuple[str, str, str], dict] = {}
    for cell in cells:
        key = (cell["case_id"], cell["peer_id"], cell["profile_id"])
        index[key] = cell

    profiles = sorted({c["profile_id"] for c in cells}) or ["forward-only"]
    case_ids = sorted({c["case_id"] for c in cells}) or sorted(cases)
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
            case_ids=case_ids,
            profiles=profiles,
            index=index,
            generated_at=generated_at,
            provenance=provenance,
            fp=fp,
        )

    hub: list[str] = [
        "# Interop",
        "",
        "Published correctness results for DNSConduit against peer DNS software "
        "under test. Results are split **by publisher** (alphabetical) so version "
        "and product matrices stay readable. No peer is preferred or recommended.",
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
    hub.extend(_provenance_table(generated_at, provenance, fp))
    hub.extend(_outcomes_legend())
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
        "The interop matrix is split **by publisher**. Start at the "
        "[interop overview](/interop/index.md).\n"
    )
    OUT_MATRIX_STUB.write_text(stub, encoding="utf-8")

    _update_mkdocs_nav(publisher_names)

    return out_page or OUT_HUB
