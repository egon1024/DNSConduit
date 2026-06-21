#!/usr/bin/env python3
"""Generate dns_wire_iana.rs from vendored IANA DNS parameter CSV files."""

from __future__ import annotations

import csv
import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
IANA_DIR = ROOT / "iana"
OUT = ROOT / "src" / "iana.rs"

# Wire number -> preferred Rhai identifier (overrides auto-normalization).
NAME_OVERRIDES: dict[str, dict[int, str]] = {
    "RecordType": {0: "ZERO"},
    "QueryClass": {254: "NONE", 255: "ANY"},
    "EdnsOptionCode": {0: "ZERO"},
}

# Extra entries not in IANA RR type registry.
EXTRA_ENTRIES: dict[str, list[tuple[int, str]]] = {
    "RecordType": [(65305, "ANAME")],
}

# Friendly Rhai aliases (name -> wire number) for Rhai modules and parse_name().
RUST_PARSE_ALIASES: dict[str, list[tuple[str, int]]] = {
    "Rcode": [("BADSIG", 16)],
    "DnsOpcode": [("DSO", 6)],
    "EdnsOptionCode": [
        ("UL", 2),
        ("CLIENT_SUBNET", 8),
        ("EXPIRE", 9),
        ("TCP_KEEPALIVE", 11),
        ("KEY_TAG", 14),
        ("EDE", 15),
        ("UMBRELLA", 20292),
    ],
}


def norm_ident(name: str) -> str | None:
    name = re.sub(r"\([^)]*\)", "", name).strip()
    ident = re.sub(r"[^A-Za-z0-9_]", "_", name.upper())
    ident = re.sub(r"_+", "_", ident).strip("_")
    if not ident:
        return None
    if ident[0].isdigit():
        ident = f"T_{ident}"
    return ident


def parse_scalar(value: str) -> int | None:
    value = value.strip()
    if not value or "-" in value:
        return None
    if not value.isdigit():
        return None
    return int(value)


def dedupe_entries(
    entries: list[tuple[int, str]],
    overrides: dict[int, str] | None = None,
) -> list[tuple[int, str]]:
    overrides = overrides or {}
    by_number: dict[int, str] = {}
    for number, ident in entries:
        if number in overrides:
            ident = overrides[number]
        if number not in by_number:
            by_number[number] = ident
    return sorted(by_number.items())


def disambiguate_reserved(entries: list[tuple[int, str]]) -> list[tuple[int, str]]:
    reserved_numbers = [n for n, ident in entries if ident == "RESERVED"]
    if len(reserved_numbers) <= 1:
        return entries
    out = []
    for number, ident in entries:
        if ident == "RESERVED" and number != 0:
            ident = f"RESERVED{number}"
        out.append((number, ident))
    return out


def load_record_types() -> list[tuple[int, str]]:
    rows = csv.DictReader((IANA_DIR / "dns-parameters-4.csv").read_text().splitlines())
    entries: list[tuple[int, str]] = []
    for row in rows:
        name = row["TYPE"].strip()
        number = parse_scalar(row["Value"].strip())
        if number is None:
            continue
        if name == "Reserved" and number == 0:
            entries.append((0, "ZERO"))
            continue
        ident = norm_ident(name)
        if ident:
            entries.append((number, ident))
    entries.extend(EXTRA_ENTRIES.get("RecordType", []))
    return dedupe_entries(entries, NAME_OVERRIDES.get("RecordType"))


def load_rcodes() -> list[tuple[int, str]]:
    rows = csv.DictReader((IANA_DIR / "dns-parameters-6.csv").read_text().splitlines())
    entries: list[tuple[int, str]] = []
    for row in rows:
        number = parse_scalar(row["RCODE"])
        if number is None:
            continue
        ident = norm_ident(row["Name"].strip())
        if ident:
            entries.append((number, ident))
    entries = dedupe_entries(entries)
    return disambiguate_reserved(entries)


def load_query_classes() -> list[tuple[int, str]]:
    rows = csv.DictReader((IANA_DIR / "dns-parameters-2.csv").read_text().splitlines())
    entries: list[tuple[int, str]] = []
    for row in rows:
        number = parse_scalar(row["Decimal"])
        if number is None:
            continue
        raw = row["Name"].strip()
        paren = re.search(r"\(([A-Z*]+)\)", raw)
        if paren:
            ident = paren.group(1).replace("*", "ANY")
        else:
            ident = norm_ident(raw)
        if ident:
            entries.append((number, ident))
    entries = dedupe_entries(entries, NAME_OVERRIDES.get("QueryClass"))
    return disambiguate_reserved(entries)


def load_opcodes() -> list[tuple[int, str]]:
    rows = csv.DictReader((IANA_DIR / "dns-parameters-5.csv").read_text().splitlines())
    entries: list[tuple[int, str]] = []
    for row in rows:
        number = parse_scalar(row["OpCode"])
        if number is None:
            continue
        ident = norm_ident(row["Name"].strip())
        if ident:
            entries.append((number, ident))
    return dedupe_entries(entries)


def load_edns_options() -> list[tuple[int, str]]:
    rows = csv.DictReader((IANA_DIR / "dns-parameters-11.csv").read_text().splitlines())
    entries: list[tuple[int, str]] = []
    for row in rows:
        number = parse_scalar(row["Value"])
        if number is None:
            continue
        raw = row["Name"].strip()
        if raw.lower() == "reserved" and number == 0:
            entries.append((0, "ZERO"))
            continue
        ident = norm_ident(raw)
        if ident:
            entries.append((number, ident))
    entries = dedupe_entries(entries, NAME_OVERRIDES.get("EdnsOptionCode"))
    # Reserved at 4 and 65535 share the same token after normalization.
    overrides = dict(NAME_OVERRIDES.get("EdnsOptionCode", {}))
    for number, ident in entries:
        if ident == "RESERVED" and number not in overrides:
            overrides[number] = f"RESERVED{number}" if number != 0 else "ZERO"
    entries = dedupe_entries(entries, overrides)
    return entries


def emit_entries(name: str, entries: list[tuple[int, str]]) -> str:
    lines = [f"pub const {name}: &[WireEnumEntry] = &["]
    for number, ident in entries:
        lines.append(f'    WireEnumEntry {{ number: {number}, name: "{ident}" }},')
    lines.append("];")
    return "\n".join(lines)


def emit_aliases(name: str, aliases: list[tuple[str, int]]) -> str:
    if not aliases:
        return f"pub const {name}: &[(&str, u16)] = &[];"
    lines = [f"pub const {name}: &[(&str, u16)] = &["]
    for alias, number in aliases:
        lines.append(f'    ("{alias}", {number}),')
    lines.append("];")
    return "\n".join(lines)


def main() -> None:
    record_types = load_record_types()
    rcodes = load_rcodes()
    query_classes = load_query_classes()
    opcodes = load_opcodes()
    edns = load_edns_options()

    sections = [
        "// @generated by scripts/generate_dns_wire_iana.py — do not edit by hand.",
        "",
        "use super::WireEnumEntry;",
        "",
        emit_entries("KNOWN_RECORD_TYPES", record_types),
        "",
        emit_aliases("RECORD_TYPE_PARSE_ALIASES", []),
        "",
        emit_entries("KNOWN_RCODES", rcodes),
        "",
        emit_aliases("RCODE_PARSE_ALIASES", RUST_PARSE_ALIASES["Rcode"]),
        "",
        emit_entries("KNOWN_QUERY_CLASSES", query_classes),
        "",
        emit_aliases("QUERY_CLASS_PARSE_ALIASES", []),
        "",
        emit_entries("KNOWN_DNS_OPCODES", opcodes),
        "",
        emit_aliases("DNS_OPCODE_PARSE_ALIASES", RUST_PARSE_ALIASES["DnsOpcode"]),
        "",
        emit_entries("KNOWN_EDNS_OPTION_CODES", edns),
        "",
        emit_aliases("EDNS_OPTION_CODE_PARSE_ALIASES", RUST_PARSE_ALIASES["EdnsOptionCode"]),
        "",
        "// Entry counts (for sanity checks): "
        f"RecordType={len(record_types)}, Rcode={len(rcodes)}, "
        f"QueryClass={len(query_classes)}, DnsOpcode={len(opcodes)}, "
        f"EdnsOptionCode={len(edns)}",
    ]

    OUT.write_text("\n".join(sections) + "\n")
    print(f"Wrote {OUT}")
    print(
        f"RecordType={len(record_types)} Rcode={len(rcodes)} "
        f"QueryClass={len(query_classes)} DnsOpcode={len(opcodes)} "
        f"EdnsOptionCode={len(edns)}"
    )


if __name__ == "__main__":
    main()
