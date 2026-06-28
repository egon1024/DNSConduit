# Unreleased

## New features

- **Data source load-safety limits** — new `data_source_limits` block caps how much `data_sources` tables can load: `max_file_bytes`, `max_entries`, `max_key_bytes`, `max_value_bytes`, `max_tables`, and aggregate `max_total_bytes`. Individual `data_sources` entries can tighten their own caps with per-entry `max_file_bytes` / `max_entries` / `max_key_bytes` / `max_value_bytes`. `max_tables` is checked in `validate`; the byte/entry caps are enforced when the snapshot is built. See [Load-safety limits](/rhai/data-sources-and-lookups.md#load-safety-limits).

## Improvements

- **Consistent backend label across the API** — the configured backend `name` (when set, else `ip:port`) is now used everywhere a backend is reported: logs, traces, and event-sink `backend` filters, plus a new Rhai `txn.selected_backend_name()` method and a `backend_name` field on the response map. `txn.selected_backend()` still returns the raw socket address. See [Transaction API](/rhai/transaction-api.md#txnselected_backend_name) and [Event export](/observability/event-export.md).

## Upgrade notes

- `data_source_limits` is optional; omitting it keeps built-in defaults, so existing configs are unaffected. Set it (or per-entry caps) if you load large lookup tables.
- Event-sink `backend` filters now match the backend **label** — the configured `name` when set, otherwise `ip:port`. A filter written as a bare `ip:port` will not match a backend that has a `name`; update such filters to the backend's `name`.
