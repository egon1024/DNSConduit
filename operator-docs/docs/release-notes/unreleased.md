# Unreleased

_Staging area for phase 1d (`dataplane-runtime-models`). Refine wording when each doc piece merges; remove `_Staging:_` markers at release cut._

## In development

- **Dataplane runtime models** — pluggable execution at startup: `sync` (default, unchanged behavior) and opt-in `split_io` (ingress, policy, and I/O worker pools with suspend/resume). Changing `dataplane.runtime` requires a process restart. _Staging: lands with architecture + config schema docs (checkpoint **A**)._

## New features

<!-- Checkpoint A — after Task 12.1 + 12.2 approved -->
<!-- - **Dataplane runtime** — `dataplane.runtime: sync | split_io`; `policy_workers`, `io_workers`, optional `slot_chunk_size`. See [Dataplane](/reference/config-schema/dataplane.md) and [Architecture and packet path](/concepts/architecture-and-packet-path.md). -->

<!-- Checkpoint B — after Task 12.3, 12.4, 12.10, 12.11 approved -->
<!-- - **Per-listener ingress overrides** — optional `threads`, `reuse_port`, `name`, and `rcvbuf` on each listener entry. See [Listeners](/reference/config-schema/listeners.md). -->
<!-- - **Backend names** — optional `name` on pool backends for stable metrics labels and overlay patches without repeating `address`. See [Pools](/reference/config-schema/pools.md). -->

## Improvements

<!-- Checkpoint C — after Task 12.5 + 12.6 approved -->
<!-- - **Overlay merge by backend name** — control-plane patches can target backends by `(pool, name)` when `name` is set; unknown names are rejected. See [Configuration model](/control-plane/configuration-model.md). -->
<!-- - **Slot pool metrics** — `conduit_slots_in_use`, `conduit_slots_capacity`, `conduit_slot_pool_exhausted_total`; `conduit_forward_outstanding` counts parked upstream waits under `split_io`. See [Built-in metrics](/observability/built-in-metrics.md). -->

<!-- Checkpoint D — after Task 12.7 + 12.8 approved -->
<!-- Guides and glossary entries for runtime tuning and terminology. -->

<!-- Checkpoint E — after Task 12.9 approved -->
<!-- Troubleshooting entries for slot exhaustion and `split_io` misconfiguration. -->

## Upgrade notes

<!-- Final — Task 13.2, pre-tag -->
<!-- - Omitted `dataplane:` block behaves as today (`sync`). Production deployments that need ingress concurrency during slow upstreams should evaluate `split_io` with tuned `listeners.threads` / `policy_workers` and `reuse_port` where appropriate. -->
<!-- - After upgrading, run `conduitctl validate --file …` and **restart** to change `dataplane.runtime` or worker counts. -->
