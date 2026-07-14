"""Inputs fingerprint for PR freshness gate."""

from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path
from typing import Iterable

from .paths import CATALOG, COMPOSE_CELL, FIXTURES, INTEROP, ROOT

# Paths whose content affects matrix meaning (relative to repo root).
FINGERPRINT_GLOBS = (
    "interop/catalog/**",
    "interop/fixtures/**",
    "interop/compose/**",
    "interop/runner/**/*.py",
    "interop/results/schema.json",
)

# Product surfaces the matrix claims to cover — changes require refresh.
# Fingerprint itself only hashes harness inputs; the gate script also watches
# these paths for "interop-relevant" PR diffs.
RELEVANT_PRODUCT_PREFIXES = (
    "crates/conduit-dataplane/",
    "crates/conduit-core/",
    "crates/conduit-config/",
    "crates/conduit/",
    "proto/conduit/v1/",
    "interop/",
)


def _iter_fingerprint_files() -> list[Path]:
    files: set[Path] = set()
    for pattern in FINGERPRINT_GLOBS:
        files.update(ROOT.glob(pattern))
    # Exclude caches / pyc
    return sorted(
        p for p in files if p.is_file() and "__pycache__" not in p.parts and not p.name.endswith(".pyc")
    )


def file_digest(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


def git_head() -> str:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            text=True,
            stderr=subprocess.DEVNULL,
        )
        return out.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return "unknown"


def compute_inputs_fingerprint(extra: Iterable[str] | None = None) -> str:
    """
    Stable sha256 over sorted relative paths and content digests of harness inputs,
    plus optional extra strings (e.g. conduit version label).
    """
    h = hashlib.sha256()
    for path in _iter_fingerprint_files():
        rel = path.relative_to(ROOT).as_posix()
        h.update(rel.encode())
        h.update(b"\0")
        h.update(file_digest(path).encode())
        h.update(b"\0")
    if extra:
        for item in extra:
            h.update(item.encode())
            h.update(b"\0")
    return f"sha256:{h.hexdigest()}"


def fingerprint_report() -> dict:
    files = _iter_fingerprint_files()
    return {
        "inputs_fingerprint": compute_inputs_fingerprint(),
        "git_head": git_head(),
        "file_count": len(files),
        "files": [p.relative_to(ROOT).as_posix() for p in files],
    }
