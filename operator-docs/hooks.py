"""MkDocs hooks — set Material header version label from env or .doc-version file."""

from __future__ import annotations

import os
from pathlib import Path

_DOC_VERSION_FILE = Path(__file__).resolve().parent / ".doc-version"


def _resolve_doc_version() -> str:
    env = os.environ.get("DOCS_PRODUCT_VERSION", "").strip()
    if env:
        return env
    if _DOC_VERSION_FILE.is_file():
        return _DOC_VERSION_FILE.read_text(encoding="utf-8").strip()
    return "development"


def on_config(config, **kwargs):
    """Expose version in header via Material extra.version.default (no dropdown)."""
    version = _resolve_doc_version()
    extra = dict(config.extra)
    version_extra = dict(extra.get("version", {}))
    version_extra["default"] = version
    # No provider — static label only (see Material "Custom version" docs).
    extra["version"] = version_extra
    config.extra = extra
