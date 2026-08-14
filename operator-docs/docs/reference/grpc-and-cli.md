# Reference: gRPC and CLI

Automation and operators map **`conduitctl`** subcommands onto gRPC services on the control listener. Connection, authentication, TLS, and command help: [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md). Proto sources live under `proto/conduit/v1/` in the repository (`control.proto`, `config.proto`, `health.proto`, `pools.proto`, and the other capability protos).

## Service: `ConduitControl`

Document-centric RPCs (apply / export / reload / validate / …).

| RPC {: .column-no-wrap } | Request | Response | Notes |
|-----|---------|----------|-------|
| `GetConfig` | `GetConfigRequest` (empty) | `effective` [`Config`](/reference/config-schema/index.md) | Current [effective config](/glossary/index.md#effective-config) |
| `ValidateConfig` | `config` [`Config`](/reference/config-schema/index.md) | `ok`, `errors[]` | Structural validation only; does not read external paths |
| `ApplyConfig` | `overlay`, `mode` | `ok`, `errors[]`, `generation`, `notes[]` | See [OverlayApplyMode](#overlayapplymode) and [status fields](#apply-status-fields) |
| `ExportConfig` | `format` | `body` | **`format` must be `yaml`** (or empty → yaml). JSON is not implemented. |
| `ReloadFromFile` | (empty) | `ok`, `errors[]`, `generation`, `notes[]` | [Reload from disk](/glossary/index.md#reload-from-disk) |
| `Health` | (empty) | `status` (`serving`) | Liveness |
| `GetTrace` | `txn_id` (decimal string) | `found`, `events[]` | Pipeline trace when enabled |
| `CheckAcl` | `ip`, optional `listener` | `ip`, `results[]` | Read-only ACL dry-run against the live snapshot; no metrics or denial logs |

Mutating RPCs leave the prior [runtime snapshot](/glossary/index.md#runtime-snapshot) unchanged when `ok` is false.

!!! note "`Health` is process liveness only"
    `Health` reports that the control plane is serving; it does **not** report upstream [backend](/glossary/index.md#backend) health. Per-backend health uses the separate **`BackendHealth`** service — see [BackendHealth service](#service-backendhealth) below and [Backend health](/policy-routing/backend-health.md).

## Apply status fields

Successful **`ApplyConfig`**, **`ReloadFromFile`**, and mutating config-primitive responses include:

| Field | Meaning |
|-------|---------|
| `generation` | Resulting configuration generation (correlates with [`conduit_config_generation`](/observability/built-in-metrics.md#conduit_config_generation)); `0` when rejected |
| `notes[]` | Extensible `kind` + `message` effect / pending-reconcile notes (proto3 additive). Empty notes are normal for fully hot applies; clients must tolerate unknown kinds |

## Service: `BackendHealth`

Proto source: `proto/conduit/v1/health.proto`. Operator commands: [gRPC and conduitctl — health](/control-plane/grpc-and-conduitctl.md#health). **Runtime control** — does not rewrite effective config or appear in **`export`**.

| RPC {: .column-no-wrap } | Request | Response | Notes |
|-----|---------|----------|-------|
| `GetBackendHealth` | optional `filter` (`pool`, `backend`) | `entries[]` | Per-backend observed/applied health, scope, eligibility, latency EWMA |
| `SetHealthControl` | `scope`, `action` | `results[]` | Freeze, manual up/down, or resume automatic |

### `BackendHealthEntry` fields

| Field | Meaning |
|-------|---------|
| `pool`, `backend` | Pool name and backend label (`name` or `address`) |
| `observed`, `applied` | `unknown`, `up`, or `down` |
| `scope_state` | Resolved scope: `inherit`, `frozen`, or `automatic` |
| `eligible` | Whether Route would select this backend now |
| `latency_ewma_ms` | Optional probe latency EWMA |
| `last_transition_unix_ms` | Optional Unix ms of last health transition |

### `SetHealthControl` actions

| Action | Effect |
|--------|--------|
| `freeze` | [Freeze](/glossary/index.md#freeze) — stop probe-driven changes to `applied` at the scope |
| `set_up` / `set_down` | Set `applied` and imply [freeze](/glossary/index.md#freeze) ([drain](/glossary/index.md#drain) = `set_down`) |
| `resume_automatic` | Unfreeze and snap `applied := observed` |

`scope.level`: `backend`, `pool`, or `global`; optional `pool` and `backend` identify the target. `backend` may be the configured `name` or `host:port` address.

## Config primitive services

Capability-oriented services for **overlay-hot** config. Mutating RPCs return the same [`ApplyConfigResponse`](#apply-status-fields) shape (`ok` / `errors` / `generation` / `notes`). Document RPCs remain on **`ConduitControl`**.

| Service {: .column-no-wrap } | Proto | `conduitctl` | RPCs (summary) |
|---------|-------|--------------|----------------|
| `ConduitPools` | `pools.proto` | `pool`, `backend` | `ListPools`, `GetPool`, `SetBackendWeight`, `AddBackend`, `RemoveBackend` |
| `ConduitOrchestrator` | `orchestrator.proto` | `orchestrator` | `GetOrchestrator`, `SetOrchestratorLimits` (`max_attempts` / `max_txn_duration_ms` only) |
| `ConduitDataSources` | `data_sources.proto` | `data-source`, `data-source-limits` | List/Get/Upsert/Remove; Get/Set limits |
| `ConduitEvents` | `events.proto` | `events` | GetEvents; GetEventSink; SetEventSinkFilters; SetEventSinkEmit (existing sinks) |
| `ConduitRhai` | `rhai.proto` | `rhai` | `GetRhai`, `SetRhaiLimits` |
| `ConduitMetrics` | `metrics_control.proto` | `metrics` | `GetMetrics`, `PatchMetrics` |
| `ConduitCaches` | `caches.proto` | `cache` | List/Get; SetCacheMaxEntries; SetCacheLmdbHot; SetCachePolicyHot |

Restart-pending fields are omitted from these RPCs (for example `txn_table_capacity`, event sink lifecycle / `queue_depth`, `memory.shard_count`). See [gRPC and conduitctl — document apply vs typed primitives](/control-plane/grpc-and-conduitctl.md#document-apply-vs-typed-primitives).

## OverlayApplyMode

Used by **`ApplyConfig`**. **`OVERLAY_APPLY_MODE_UNSPECIFIED` (0)** is treated as **merge**.

| Enum {: .column-no-wrap } | Value | `conduitctl` equivalent | `overlay` field |
|------|-------|-------------------------|-----------------|
| `OVERLAY_APPLY_MODE_UNSPECIFIED` | 0 | default **merge** | Patch required |
| `OVERLAY_APPLY_MODE_MERGE` | 1 | default or `--merge` | Patch required |
| `OVERLAY_APPLY_MODE_REPLACE` | 2 | `--replace` | Patch required; empty patch clears overlay |
| `OVERLAY_APPLY_MODE_CLEAR` | 3 | `--clear` | Omit / ignore |

### ApplyConfig (conceptual)

```protobuf
message ApplyConfigRequest {
  Config overlay = 1;
  OverlayApplyMode mode = 2;
}
message ApplyConfigResponse {
  bool ok = 1;
  repeated string errors = 2;
  uint64 generation = 3;
  repeated ConfigApplyStatusNote notes = 4;
}
```

Overlay patches must not include **`rules`** or **`tracing`** — the server rejects them. **`metrics`** is allowed (deep merge). Allowed sections match [Configuration model — overlay merge](/control-plane/configuration-model.md#how-file-and-overlay-merge). Pool/backend **`remove: true`**: [Remove marker](/control-plane/configuration-model.md#remove-marker).

## CLI mapping

| `conduitctl` | RPC | Local only? |
|--------------|-----|-------------|
| `apply` | `ApplyConfig` | No |
| `export` | `ExportConfig` (`format: yaml`) | No |
| `reload` | `ReloadFromFile` | No |
| `validate --file` | — | **Yes** (does not call `ValidateConfig`) |
| `acl check` | `CheckAcl` | No (default) |
| `acl check --file` | — | **Yes** (offline compile; does not call `CheckAcl`) |
| `trace` | `GetTrace` | No |
| `health show` | `GetBackendHealth` | No |
| `health freeze` | `SetHealthControl` (`freeze`) | No |
| `health set` | `SetHealthControl` (`set_up` / `set_down`) | No |
| `health resume` | `SetHealthControl` (`resume_automatic`) | No |
| `pool list` / `pool get` | `ListPools` / `GetPool` | No |
| `backend set-weight` / `remove` | `SetBackendWeight` / `RemoveBackend` | No |
| `orchestrator get` / `set-limits` | `GetOrchestrator` / `SetOrchestratorLimits` | No |
| `data-source …` / `data-source-limits …` | `ConduitDataSources` | No |
| `events …` | `ConduitEvents` | No |
| `rhai get` / `set-limits` | `GetRhai` / `SetRhaiLimits` | No |
| `metrics get` / `patch` | `GetMetrics` / `PatchMetrics` | No |
| `cache …` | `ConduitCaches` | No |

Global client flags and YAML client config: [gRPC and conduitctl — connecting](/control-plane/grpc-and-conduitctl.md#connecting).

## GetTrace event fields

Each `TraceEvent` in the response includes:

| Field | Meaning |
|-------|---------|
| `phase` | Pipeline phase name |
| `elapsed_us` | Microseconds since transaction start |
| `message` | Optional detail |
| `pool` | Optional pool name |
| `backend` | Optional backend address |
| `cache` | Optional named cache instance (nested cache provider events) |

## Related topics

- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md) — enable control, connect, authenticate, TLS, primitives
- [Reference: control](/reference/config-schema/control.md) — `control:` config block
- [Reload and export](/control-plane/reload-and-export.md) — operator workflows
