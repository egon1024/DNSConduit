# Manual tests — pluggable lookup system

**OpenSpec:** `pluggable-lookup-system`  
**Status:** Stub — Gate A labs added as phases land.

## Port map

| Service | Address |
|---------|---------|
| Conduit UDP listener | `127.0.0.1:15353` |
| Upstream resolver (fixture) | `127.0.0.1:5300` |
| Prometheus (when enabled) | per config |

## Gate A — Config freeze

- [ ] `conduitctl validate` passes on `tests/fixtures/config/lookup-forward-only.yaml`
- [ ] `conduitctl validate` passes on `tests/fixtures/config/lookup-cache-enabled.yaml`
- [ ] `conduitctl validate` fails on `lookup-invalid-cache-ref.yaml`, `lookup-invalid-on-hit.yaml`, `lookup-invalid-truncated-ttl.yaml`
- [ ] Export/round-trip preserves `lookup` and `caches` blocks

## Gate B — Forward parity

_(Placeholder — add labs when Lookup spine ships.)_

## Gate C — Cache fast path

_(Placeholder.)_

## Gate D — Cache policy

_(Placeholder.)_

## Gate E — Observability

- [ ] Cache hit: built-in hit counter + `answer_source=cache` on responses; no forward attempt counters
- [ ] Cache miss: forward series + `answer_source=forward`
- [ ] PromQL: compare volume/latency by `answer_source` (full profile)
- [ ] OTLP parity spot-check for lookup/cache series
- [ ] Rhai `txn.answer_source()` returns `cache` when `on_hit.response_rules: run` on hit

## Gate G — Documentation

- [ ] Operator-docs: `on_hit.response_rules` skip vs run and custom metrics
- [ ] Operator-docs: `last_forward_ms()` zero on cache hits; use `answer_source()`
- [ ] Built-in metrics catalog updated for lookup/cache
