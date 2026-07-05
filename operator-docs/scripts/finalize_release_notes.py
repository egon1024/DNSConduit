#!/usr/bin/env python3
"""Promote release-notes/unreleased.md to a versioned page at release cut."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

EMPTY_UNRELEASED = """# Unreleased

_No unreleased operator-facing changes._
"""

PLACEHOLDER_PATTERNS = (
    re.compile(r"^#\s*Unreleased\s*$", re.IGNORECASE),
    re.compile(r"^_\s*No unreleased operator-facing changes\.\s*_$"),
    re.compile(r"^_\s*No published release notes yet\..*_$"),
)

INDEX_TABLE_MARKER = "| --- | --- | --- |"
INDEX_EMPTY_NOTE = (
    "_No published release notes yet. The first entry appears when the next version ships._"
)

ISO_DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")

DEFAULT_RELEASE_DATES_JSON = Path("operator-docs/site-root/versions/release-dates.json")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True, help="Release semver (e.g. 0.14.0)")
    parser.add_argument(
        "--release-date",
        required=True,
        help="Release date in ISO form YYYY-MM-DD (UTC recommended)",
    )
    parser.add_argument(
        "--release-notes-dir",
        type=Path,
        default=Path("operator-docs/docs/release-notes"),
        help="Directory containing index.md, unreleased.md, and version pages",
    )
    parser.add_argument(
        "--release-dates-json",
        type=Path,
        default=DEFAULT_RELEASE_DATES_JSON,
        help="Static map of version to ISO date for the global Versions page",
    )
    parser.add_argument(
        "--repo",
        default="https://github.com/egon1024/DNSConduit",
        help="Repository URL for GitHub release links (no trailing slash)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print outputs without writing files",
    )
    return parser.parse_args()


def normalize_repo_url(repo: str) -> str:
    return repo.rstrip("/")


def release_tag_url(repo: str, version: str) -> str:
    return f"{normalize_repo_url(repo)}/releases/tag/{version}"


def validate_iso_date(release_date: str) -> str:
    if not ISO_DATE_RE.fullmatch(release_date):
        raise SystemExit(f"Invalid release date (expected YYYY-MM-DD): {release_date}")
    return release_date


def semver_sort_key(version: str) -> tuple[int, ...]:
    nums = version.split("-", 1)[0].split(".")
    return tuple(int(n) for n in nums)


def strip_unreleased_header(text: str) -> str:
    lines = text.splitlines()
    body: list[str] = []
    skipped_title = False
    for line in lines:
        if not skipped_title and PLACEHOLDER_PATTERNS[0].match(line.strip()):
            skipped_title = True
            continue
        if any(pattern.match(line.strip()) for pattern in PLACEHOLDER_PATTERNS[1:]):
            continue
        body.append(line)
    return "\n".join(body).strip()


def has_substantive_unreleased(text: str) -> bool:
    body = strip_unreleased_header(text)
    if not body:
        return False
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        if stripped.startswith("##") or stripped.startswith("-") or stripped.startswith("*"):
            return True
        if stripped.startswith(">"):
            return True
        # Any other non-empty prose counts as substantive.
        return True
    return False


def first_summary_line(body: str) -> str:
    for line in body.splitlines():
        stripped = line.strip()
        if stripped.startswith("- ") or stripped.startswith("* "):
            summary = stripped[2:].strip()
            if summary:
                return _truncate_summary(summary)
        if stripped.startswith("## "):
            heading = stripped[3:].strip()
            if heading:
                return _truncate_summary(heading)
    return "See release notes"


def _truncate_summary(text: str, limit: int = 120) -> str:
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", text)
    if len(text) <= limit:
        return text
    return text[: limit - 1].rstrip() + "…"


def render_version_page(
    version: str,
    release_date: str,
    body: str,
    repo: str,
    *,
    stub: bool,
) -> str:
    tag_url = release_tag_url(repo, version)
    lines = [
        f"# Release notes — {version}",
        "",
        f"Released **{release_date}** with DNS Conduit **{version}**.",
        "",
    ]
    if stub:
        lines.extend(
            [
                "Maintenance release: bug fixes and internal improvements with no documented "
                "operator-facing changes.",
                "",
            ]
        )
    else:
        lines.append(body)
        lines.append("")

    lines.extend(
        [
            "---",
            "",
            f"[All changes in this release]({tag_url}) (automated pull request list).",
            "",
        ]
    )
    return "\n".join(lines)


def insert_index_row(
    index_text: str,
    version: str,
    release_date: str,
    summary: str,
) -> str:
    if f"]({version}.md)" in index_text:
        return index_text

    row = f"| [{version}]({version}.md) | {release_date} | {summary} |"
    if INDEX_TABLE_MARKER not in index_text:
        raise SystemExit(f"index.md missing releases table marker: {INDEX_TABLE_MARKER}")

    marker_pos = index_text.index(INDEX_TABLE_MARKER)
    insert_at = index_text.index("\n", marker_pos) + 1
    updated = index_text[:insert_at] + row + "\n" + index_text[insert_at:]
    if INDEX_EMPTY_NOTE in updated:
        updated = updated.replace(INDEX_EMPTY_NOTE, "", 1).replace("\n\n\n", "\n\n")
    return updated


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


def merge_release_date(path: Path, version: str, release_date: str) -> str:
    dates = load_release_dates(path)
    dates[version] = validate_iso_date(release_date)
    return write_release_dates(path, dates)


def finalize(
    version: str,
    release_date: str,
    release_notes_dir: Path,
    repo: str,
    *,
    release_dates_json: Path | None = DEFAULT_RELEASE_DATES_JSON,
    dry_run: bool = False,
) -> bool:
    """Return True if a new version page was written."""
    release_date = validate_iso_date(release_date)
    unreleased_path = release_notes_dir / "unreleased.md"
    index_path = release_notes_dir / "index.md"
    version_path = release_notes_dir / f"{version}.md"

    if not unreleased_path.is_file():
        raise SystemExit(f"Missing {unreleased_path}")
    if not index_path.is_file():
        raise SystemExit(f"Missing {index_path}")

    if version_path.is_file():
        print(f"Release notes for {version} already exist; skipping finalize.")
        return False

    unreleased_text = unreleased_path.read_text(encoding="utf-8")
    substantive = has_substantive_unreleased(unreleased_text)
    body = strip_unreleased_header(unreleased_text)
    summary = "Maintenance release" if not substantive else first_summary_line(body)

    version_page = render_version_page(
        version,
        release_date,
        body,
        repo,
        stub=not substantive,
    )
    index_page = insert_index_row(
        index_path.read_text(encoding="utf-8"),
        version,
        release_date,
        summary,
    )
    dates_json_text = None
    if release_dates_json is not None:
        if dry_run:
            dates = load_release_dates(release_dates_json)
            dates[version] = release_date
            dates_json_text = json.dumps(
                dict(
                    sorted(
                        dates.items(),
                        key=lambda item: semver_sort_key(item[0]),
                        reverse=True,
                    )
                ),
                indent=2,
            ) + "\n"
        else:
            dates_json_text = merge_release_date(release_dates_json, version, release_date)

    if dry_run:
        print(version_page)
        print("--- index ---")
        print(index_page)
        if dates_json_text is not None:
            print("--- release-dates.json ---")
            print(dates_json_text, end="")
        return True

    version_path.write_text(version_page, encoding="utf-8")
    index_path.write_text(index_page, encoding="utf-8")
    unreleased_path.write_text(EMPTY_UNRELEASED, encoding="utf-8")
    print(f"Wrote {version_path}")
    print(f"Updated {index_path}")
    if release_dates_json is not None:
        print(f"Updated {release_dates_json}")
    print(f"Reset {unreleased_path}")
    return True


def main() -> None:
    args = parse_args()
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version):
        raise SystemExit(f"Invalid semver: {args.version}")

    wrote = finalize(
        args.version,
        args.release_date,
        args.release_notes_dir,
        args.repo,
        release_dates_json=args.release_dates_json,
        dry_run=args.dry_run,
    )
    if not wrote:
        sys.exit(0)


if __name__ == "__main__":
    main()
