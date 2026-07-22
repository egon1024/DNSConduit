#!/usr/bin/env python3
"""Merge release-dates.json maps for the global Versions page deploy.

Docs deploy must not hard-code a git branch as the dates source of truth.
Each release finalize updates the map on the release line and tags it; deploys
then union that checkout into the already-published gh-pages map so:

- New releases from any release line (``1.x``, ``2.x``, ``main``, …) publish dates
- Redeploying an older tag cannot drop newer versions' dates already on the site

Later ``--input`` files win on key conflicts. Missing inputs are skipped.

This script is intentionally self-contained (no imports from sibling scripts) so
docs-deploy can ``git show ${GITHUB_SHA}:…`` it even when the product tag
checkout predates the helper.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ISO_DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def validate_iso_date(release_date: str) -> str:
    if not ISO_DATE_RE.fullmatch(release_date):
        raise SystemExit(f"Invalid release date (expected YYYY-MM-DD): {release_date}")
    return release_date


def semver_sort_key(version: str) -> tuple[int, ...]:
    nums = version.split("-", 1)[0].split(".")
    return tuple(int(n) for n in nums)


def load_release_dates(path: Path) -> dict[str, str]:
    if not path.is_file():
        return {}
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise SystemExit(f"Invalid release dates JSON (expected object): {path}")
    dates: dict[str, str] = {}
    for key, value in data.items():
        dates[str(key)] = validate_iso_date(str(value))
    return dates


def write_release_dates(path: Path, dates: dict[str, str]) -> str:
    ordered = dict(
        sorted(
            dates.items(),
            key=lambda item: semver_sort_key(item[0]),
            reverse=True,
        )
    )
    text = json.dumps(ordered, indent=2) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return text


def merge_release_dates_maps(*maps: dict[str, str]) -> dict[str, str]:
    """Left-to-right union; later maps overwrite the same version key."""
    merged: dict[str, str] = {}
    for layer in maps:
        merged.update(layer)
    return merged


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--input",
        action="append",
        default=[],
        metavar="PATH",
        help="JSON file to merge (repeatable; later wins on conflicts). Missing files ignored.",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="Write merged release-dates.json here",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    layers: list[dict[str, str]] = []
    for raw in args.input:
        path = Path(raw)
        if not path.is_file():
            print(f"skip missing release-dates input: {path}", file=sys.stderr)
            continue
        layers.append(load_release_dates(path))
    merged = merge_release_dates_maps(*layers)
    write_release_dates(args.output, merged)
    print(f"Wrote {len(merged)} release date(s) to {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
