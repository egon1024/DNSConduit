#!/usr/bin/env python3
"""Backfill ISO release dates into release notes and release-dates.json.

Fetches publishedAt from GitHub Releases when --repo is set (requires gh CLI).
Use --dates JSON or repeat --date VERSION=YYYY-MM-DD to supply dates manually.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

from finalize_release_notes import (
    DEFAULT_RELEASE_DATES_JSON,
    load_release_dates,
    merge_release_date,
    validate_iso_date,
    write_release_dates,
)

RELEASE_NOTES_DIR = Path("operator-docs/docs/release-notes")
VERSION_LEAD_OLD = re.compile(
    r"^Released with DNS Conduit \*\*([0-9]+\.[0-9]+\.[0-9]+)\*\*\.\s*$"
)
VERSION_LEAD_NEW = re.compile(
    r"^Released \*\*(\d{4}-\d{2}-\d{2})\*\* with DNS Conduit "
    r"\*\*([0-9]+\.[0-9]+\.[0-9]+)\*\*\.\s*$"
)
INDEX_ROW = re.compile(
    r"^\| \[([0-9]+\.[0-9]+\.[0-9]+)\]\(\1\.md\) \|(?: ([0-9]{4}-[0-9]{2}-[0-9]{2}) \|)? (.*) \|$"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo",
        default="egon1024/DNSConduit",
        help="GitHub owner/repo for gh release list (omit with --dates only)",
    )
    parser.add_argument(
        "--dates",
        help='Inline JSON object of version to date, e.g. \'{"0.14.0":"2026-06-23"}\'',
    )
    parser.add_argument(
        "--date",
        action="append",
        default=[],
        metavar="VERSION=YYYY-MM-DD",
        help="Manual date entry (repeatable)",
    )
    parser.add_argument(
        "--release-notes-dir",
        type=Path,
        default=RELEASE_NOTES_DIR,
        help="Release notes directory",
    )
    parser.add_argument(
        "--release-dates-json",
        type=Path,
        default=DEFAULT_RELEASE_DATES_JSON,
        help="Static release-dates.json path",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print planned changes without writing files",
    )
    return parser.parse_args()


def gh_release_dates(repo: str) -> dict[str, str]:
    cmd = [
        "gh",
        "release",
        "list",
        "--repo",
        repo,
        "--limit",
        "200",
        "--json",
        "tagName,publishedAt",
    ]
    try:
        raw = subprocess.check_output(cmd, text=True)
    except (subprocess.CalledProcessError, FileNotFoundError) as exc:
        raise SystemExit(f"Failed to fetch GitHub releases via gh: {exc}") from exc

    dates: dict[str, str] = {}
    for entry in json.loads(raw):
        tag = str(entry["tagName"])
        published = str(entry["publishedAt"])
        if not published:
            continue
        dates[tag] = validate_iso_date(published[:10])
    return dates


def parse_manual_dates(args: argparse.Namespace) -> dict[str, str]:
    dates: dict[str, str] = {}
    if args.dates:
        loaded = json.loads(args.dates)
        if not isinstance(loaded, dict):
            raise SystemExit("--dates must be a JSON object")
        for key, value in loaded.items():
            dates[str(key)] = validate_iso_date(str(value))
    for item in args.date:
        if "=" not in item:
            raise SystemExit(f"Invalid --date (expected VERSION=YYYY-MM-DD): {item}")
        version, release_date = item.split("=", 1)
        dates[version.strip()] = validate_iso_date(release_date.strip())
    return dates


def resolve_dates(args: argparse.Namespace) -> dict[str, str]:
    manual = parse_manual_dates(args)
    if manual and args.repo:
        gh_dates = gh_release_dates(args.repo)
        gh_dates.update(manual)
        return gh_dates
    if manual:
        return manual
    if args.repo:
        return gh_release_dates(args.repo)
    raise SystemExit("Provide --repo and/or manual --dates / --date entries")


def update_version_page(path: Path, version: str, release_date: str, *, dry_run: bool) -> bool:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    changed = False
    for idx, line in enumerate(lines):
        new_match = VERSION_LEAD_NEW.match(line)
        if new_match:
            if new_match.group(1) == release_date and new_match.group(2) == version:
                return False
            lines[idx] = f"Released **{release_date}** with DNS Conduit **{version}**."
            changed = True
            break
        old_match = VERSION_LEAD_OLD.match(line)
        if old_match and old_match.group(1) == version:
            lines[idx] = f"Released **{release_date}** with DNS Conduit **{version}**."
            changed = True
            break
    if not changed:
        return False
    updated = "\n".join(lines) + ("\n" if text.endswith("\n") else "")
    if dry_run:
        print(f"Would update {path}")
    else:
        path.write_text(updated, encoding="utf-8")
        print(f"Updated {path}")
    return True


def update_index(index_path: Path, dates: dict[str, str], *, dry_run: bool) -> bool:
    text = index_path.read_text(encoding="utf-8")
    lines = text.splitlines()
    changed = False
    out: list[str] = []
    for line in lines:
        if line.startswith("| Version | Summary |"):
            out.append("| Version | Released | Summary |")
            changed = True
            continue
        if line.startswith("| --- | --- |") and "| --- | --- | --- |" not in text:
            out.append("| --- | --- | --- |")
            changed = True
            continue
        match = INDEX_ROW.match(line)
        if match:
            version, existing_date, summary = match.groups()
            release_date = dates.get(version)
            if release_date and existing_date != release_date:
                out.append(f"| [{version}]({version}.md) | {release_date} | {summary} |")
                changed = True
                continue
            if release_date and existing_date is None:
                out.append(f"| [{version}]({version}.md) | {release_date} | {summary} |")
                changed = True
                continue
        out.append(line)

    if not changed:
        return False

    updated = "\n".join(out) + ("\n" if text.endswith("\n") else "")
    if dry_run:
        print(f"Would update {index_path}")
    else:
        index_path.write_text(updated, encoding="utf-8")
        print(f"Updated {index_path}")
    return True


def main() -> None:
    args = parse_args()
    dates = resolve_dates(args)

    notes_dir = args.release_notes_dir
    if not notes_dir.is_dir():
        raise SystemExit(f"Missing release notes directory: {notes_dir}")

    any_change = False
    for path in sorted(notes_dir.glob("[0-9]*.[0-9]*.[0-9]*.md")):
        version = path.stem
        release_date = dates.get(version)
        if not release_date:
            continue
        if update_version_page(path, version, release_date, dry_run=args.dry_run):
            any_change = True

    index_path = notes_dir / "index.md"
    if index_path.is_file() and update_index(index_path, dates, dry_run=args.dry_run):
        any_change = True

    json_path = args.release_dates_json
    existing = load_release_dates(json_path)
    merged = {**existing, **dates}

    if merged != existing:
        any_change = True
        if args.dry_run:
            print(f"Would update {json_path}")
            print(json.dumps(merged, indent=2))
        else:
            write_release_dates(json_path, merged)
            print(f"Updated {json_path}")

    if not any_change:
        print("No changes needed.")
        sys.exit(0)


if __name__ == "__main__":
    main()
