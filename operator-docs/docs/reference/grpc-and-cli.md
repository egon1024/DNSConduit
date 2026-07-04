# Reference: gRPC and CLI

Field-level reference for the **`ConduitControl`** gRPC service and how **`conduitctl`** maps to it. Connection, authentication, and operator-oriented command help: [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md). Proto source: `proto/conduit/v1/control.proto` and `proto/conduit/v1/config.proto` in the repository.

## Service: `ConduitControl`

| RPC {: .column-no-wrap } | Request | Response | Notes |
|-----|---------|----------|-------|
| `GetConfig` | `GetConfigRequest` (empty) | `effective` [`Config`](/reference/config-schema/index.md) | Current [effective config](/glossary/index.md#effective-config) |
| `ValidateConfig` | `config` [`Config`](/reference/config-schema/index.md) | `ok`, `errors[]` | Structural validation only; does not read external paths |
| `ApplyConfig` | `overlay`, `mode` | `ok`, `errors[]` | See [OverlayApplyMode](#overlayapplymode) |
| `ExportConfig` | `format` | `body` | **`format` must be `yaml`** (or empty → yaml). JSON is not implemented. |
| `ReloadFromFile` | (empty) | `ok`, `errors[]` | [Reload from disk](/glossary/index.md#reload-from-disk) |
| `Health` | (empty) | `status` (`serving`) | Liveness |
| `GetTrace` | `txn_id` (decimal string) | `found`, `events[]` | Pipeline trace when enabled |

Mutating RPCs (`ApplyConfig`, `ReloadFromFile`) leave the prior [runtime snapshot](/glossary/index.md#runtime-snapshot) unchanged when `ok` is false.

!!! note "`Health` is process liveness only"
    `Health` reports that the control plane is serving; it does **not** report upstream [backend](/glossary/index.md#backend) health. Per-backend health uses the separate **`BackendHealth`** service — see [BackendHealth service](#service-backendhealth) below and [Backend health](/policy-routing/backend-health.md).

## Service: `BackendHealth`

Proto source: `proto/conduit/v1/health.proto`. Operator commands: [gRPC and conduitctl — health](/control-plane/grpc-and-conduitctl.md#health).

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
}
```

Overlay patches must not include **`rules`**, **`metrics`**, or **`tracing`** — the server rejects them. Allowed sections match [Configuration model — overlay merge](/control-plane/configuration-model.md#how-file-and-overlay-merge).

## CLI mapping

| `conduitctl` | RPC | Local only? |
|--------------|-----|-------------|
| `apply` | `ApplyConfig` | No |
| `export` | `ExportConfig` (`format: yaml`) | No |
| `reload` | `ReloadFromFile` | No |
| `validate --file` | — | **Yes** (does not call `ValidateConfig`) |
| `trace` | `GetTrace` | No |
| `health show` | `GetBackendHealth` | No |
| `health freeze` | `SetHealthControl` (`freeze`) | No |
| `health set` | `SetHealthControl` (`set_up` / `set_down`) | No |
| `health resume` | `SetHealthControl` (`resume_automatic`) | No |

Global client flags: `--endpoint` / `CONDUIT_CONTROL`, `--api-key` / `CONDUIT_API_KEY`. See [gRPC and conduitctl — connecting](/control-plane/grpc-and-conduitctl.md#connecting).

## GetTrace event fields

Each `TraceEvent` in the response includes:

| Field | Meaning |
|-------|---------|
| `phase` | Pipeline phase name |
| `elapsed_us` | Microseconds since transaction start |
| `message` | Optional detail |
| `pool` | Optional pool name |
| `backend` | Optional backend address |

## Related topics

- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md) — enable control, connect, authenticate
- [Reference: control](/reference/config-schema/control.md) — `control:` config block
- [Reload and export](/control-plane/reload-and-export.md) — operator workflows
