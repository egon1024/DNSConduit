#!/usr/bin/env python3
"""Generate operator-docs/docs/versions.md from mike versions.json (or a stub)."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

STABLE_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--versions-json",
        type=Path,
        help="Path to mike versions.json (from gh-pages checkout)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output markdown path (e.g. docs/versions.md)",
    )
    parser.add_argument(
        "--site-url",
        default="https://egon1024.github.io/DNSConduit/",
        help="Site base URL with trailing slash",
    )
    parser.add_argument(
        "--current-version",
        default="",
        help="Version being built (tag name); shown in banner partial",
    )
    parser.add_argument(
        "--stub",
        action="store_true",
        help="Write placeholder when versions.json is missing",
    )
    return parser.parse_args()


def load_versions(path: Path | None, stub: bool) -> list[dict[str, Any]]:
    if path is None or not path.is_file():
        if stub:
            return []
        print("versions.json not found; use --stub for local/PR builds", file=sys.stderr)
        sys.exit(1)
    data = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(data, list):
        return data
    if isinstance(data, dict) and "versions" in data:
        return data["versions"]
    raise SystemExit(f"unexpected versions.json shape: {type(data)}")


def version_href(site_url: str, version: str) -> str:
    base = site_url if site_url.endswith("/") else f"{site_url}/"
    return f"{base}{version}/"


def render_versions_md(
    entries: list[dict[str, Any]], site_url: str, current_version: str
) -> str:
    lines = [
        "# Documentation versions",
        "",
        "Published snapshots of this manual, keyed to product release tags.",
        "",
    ]

    if not entries:
        lines.extend(
            [
                "_No published versions yet. After the first release tag is deployed to "
                "GitHub Pages, this page lists all available versions and aliases._",
                "",
            ]
        )
    else:
        lines.extend(["## Releases", "", "| Version | Open |", "| --- | --- |"])
        for entry in sorted(entries, key=lambda e: e.get("version", "")):
            version = entry.get("version") or entry.get("title", "")
            if not version:
                continue
            lines.append(f"| `{version}` | [View]({version_href(site_url, version)}) |")
        lines.append("")

        alias_rows: list[tuple[str, str]] = []
        for entry in entries:
            version = entry.get("version") or entry.get("title", "")
            for alias in entry.get("aliases") or []:
                alias_rows.append((alias, version))
        if alias_rows:
            lines.extend(["## Aliases", "", "| Alias | Points to | Open |", "| --- | --- | --- |"])
            for alias, version in sorted(alias_rows):
                lines.append(
                    f"| `{alias}` | `{version}` | [View]({version_href(site_url, version)}) |"
                )
            lines.append("")
            lines.extend(
                [
                    "- **`latest`** — newest stable release (`MAJOR.MINOR.PATCH` with no pre-release suffix).",
                    "- **`dev`** — newest `2.0.0-dev.N` development checkpoint (when published).",
                    "- **`stable-1`** — newest `1.x` stable release after 2.0.0 exists (when published).",
                    "",
                ]
            )

    return "\n".join(lines)


def write_doc_version_file(operator_docs_root: Path, current_version: str) -> None:
    label = current_version if current_version else "development"
    (operator_docs_root / ".doc-version").write_text(label + "\n", encoding="utf-8")


def main() -> None:
    args = parse_args()
    entries = load_versions(args.versions_json, args.stub)

    # Include the version currently being built if not yet in versions.json.
    if args.current_version and not any(
        (e.get("version") or e.get("title")) == args.current_version for e in entries
    ):
        aliases: list[str] = []
        if STABLE_RE.match(args.current_version):
            aliases = []  # alias assignment happens in deploy workflow
        elif "-dev." in args.current_version:
            aliases = ["dev"]
        entries.append(
            {
                "version": args.current_version,
                "title": args.current_version,
                "aliases": aliases,
            }
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        render_versions_md(entries, args.site_url, args.current_version),
        encoding="utf-8",
    )

    operator_docs_root = args.output.resolve().parent.parent
    write_doc_version_file(operator_docs_root, args.current_version)
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
