"""MkDocs hooks — doc version label and build-time table column nowrap."""

from __future__ import annotations

import os
import re
from pathlib import Path

_DOC_VERSION_FILE = Path(__file__).resolve().parent / ".doc-version"

_COLUMN_NO_WRAP_MARKER = "column-no-wrap"
_COLUMN_NO_WRAP_CELL = "column-no-wrap-cell"

_CELL_RE = re.compile(r"(<t[hd][^>]*>)(.*?)(</t[hd]>)", re.DOTALL | re.IGNORECASE)
_TABLE_RE = re.compile(r"<table\b[^>]*>.*?</table>", re.DOTALL | re.IGNORECASE)
_THEAD_RE = re.compile(r"<thead\b[^>]*>(.*?)</thead>", re.DOTALL | re.IGNORECASE)
_TBODY_RE = re.compile(r"<tbody\b[^>]*>(.*?)</tbody>", re.DOTALL | re.IGNORECASE)
_TR_RE = re.compile(r"<tr\b[^>]*>(.*?)</tr>", re.DOTALL | re.IGNORECASE)
_CLASS_ATTR_RE = re.compile(r'\bclass="([^"]*)"')


def _resolve_doc_version() -> str:
    env = os.environ.get("DOCS_PRODUCT_VERSION", "").strip()
    if env:
        return env
    if _DOC_VERSION_FILE.is_file():
        return _DOC_VERSION_FILE.read_text(encoding="utf-8").strip()
    return "development"


def _cell_has_class(opening_tag: str, class_name: str) -> bool:
    match = _CLASS_ATTR_RE.search(opening_tag)
    if not match:
        return False
    return class_name in match.group(1).split()


def _append_class(opening_tag: str, class_name: str) -> str:
    match = _CLASS_ATTR_RE.search(opening_tag)
    if match:
        classes = match.group(1).split()
        if class_name not in classes:
            classes.append(class_name)
        return (
            opening_tag[: match.start(1)]
            + " ".join(classes)
            + opening_tag[match.end(1) :]
        )
    return opening_tag[:-1] + f' class="{class_name}">'


def _split_row_cells(row_html: str) -> list[tuple[str, str, str]]:
    return [(m.group(1), m.group(2), m.group(3)) for m in _CELL_RE.finditer(row_html)]


def _join_row_cells(cells: list[tuple[str, str, str]]) -> str:
    return "".join(f"{opening}{content}{closing}" for opening, content, closing in cells)


def _marked_column_indices(header_row_html: str) -> list[int]:
    cells = _split_row_cells(header_row_html)
    return [
        index
        for index, (opening, _content, _closing) in enumerate(cells)
        if _cell_has_class(opening, _COLUMN_NO_WRAP_MARKER)
    ]


def _apply_indices_to_row(row_html: str, indices: list[int]) -> tuple[str, bool]:
    cells = _split_row_cells(row_html)
    if not cells:
        return row_html, False

    changed = False
    updated: list[tuple[str, str, str]] = []
    for index, (opening, content, closing) in enumerate(cells):
        if index in indices and not _cell_has_class(opening, _COLUMN_NO_WRAP_CELL):
            opening = _append_class(opening, _COLUMN_NO_WRAP_CELL)
            changed = True
        updated.append((opening, content, closing))

    if not changed:
        return row_html, False
    return _join_row_cells(updated), True


def _process_rows(section_html: str, indices: list[int]) -> tuple[str, bool]:
    changed = False

    def replace_row(match: re.Match[str]) -> str:
        nonlocal changed
        row_inner = match.group(1)
        new_inner, row_changed = _apply_indices_to_row(row_inner, indices)
        if row_changed:
            changed = True
            return match.group(0).replace(row_inner, new_inner, 1)
        return match.group(0)

    new_section = _TR_RE.sub(replace_row, section_html)
    return new_section, changed


def _process_table(table_html: str) -> str:
    thead_match = _THEAD_RE.search(table_html)
    if not thead_match:
        return table_html

    header_rows = list(_TR_RE.finditer(thead_match.group(1)))
    if not header_rows:
        return table_html

    indices = _marked_column_indices(header_rows[0].group(1))
    if not indices:
        return table_html

    result = table_html
    thead_html = thead_match.group(1)
    new_thead_html, thead_changed = _process_rows(thead_html, indices)
    if thead_changed:
        result = result.replace(thead_html, new_thead_html, 1)

    tbody_match = _TBODY_RE.search(result)
    if tbody_match:
        tbody_html = tbody_match.group(1)
        new_tbody_html, tbody_changed = _process_rows(tbody_html, indices)
        if tbody_changed:
            result = result.replace(tbody_html, new_tbody_html, 1)

    return result


def _apply_column_no_wrap(html: str) -> str:
    """Add .column-no-wrap-cell to every cell in columns marked on the header row."""
    if _COLUMN_NO_WRAP_MARKER not in html:
        return html

    def replace_table(match: re.Match[str]) -> str:
        return _process_table(match.group(0))

    return _TABLE_RE.sub(replace_table, html)


def on_config(config, **kwargs):
    """Expose docs product version in __config (read by docs/assets/javascripts/doc-version.js)."""
    version = _resolve_doc_version()
    extra = dict(config.extra)
    version_extra = dict(extra.get("version", {}))
    version_extra["default"] = version
    # No provider — static label only (see Material "Custom version" docs).
    extra["version"] = version_extra
    config.extra = extra


def on_page_content(html, *, page, config, files, **kwargs):
    """Propagate .column-no-wrap header markers to cells in the same column."""
    return _apply_column_no_wrap(html)
