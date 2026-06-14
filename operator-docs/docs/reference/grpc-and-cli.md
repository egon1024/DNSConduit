# Reference: gRPC and CLI

Field-level reference for the **`ConduitControl`** gRPC service and how **`conduitctl`** maps to it. Connection, authentication, and operator-oriented command help: [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md). Proto source: `proto/conduit/v1/control.proto` and `proto/conduit/v1/config.proto` in the repository.

## Service: `ConduitControl`

| RPC | Request | Response | Notes |
|-----|---------|----------|-------|
| `GetConfig` | `GetConfigRequest` (empty) | `effective` [`Config`](/reference/config-schema/index.md) | Current [effective config](/glossary/index.md#effective-config) |
| `ValidateConfig` | `config` [`Config`](/reference/config-schema/index.md) | `ok`, `errors[]` | Structural validation only; does not read external paths |
| `ApplyConfig` | `overlay`, `mode` | `ok`, `errors[]` | See [OverlayApplyMode](#overlayapplymode) |
| `ExportConfig` | `format` | `body` | **`format` must be `yaml`** (or empty → yaml). JSON is not implemented. |
| `ReloadFromFile` | (empty) | `ok`, `errors[]` | [Reload from disk](/glossary/index.md#reload-from-disk) |
| `Health` | (empty) | `status` (`serving`) | Liveness |
| `GetTrace` | `txn_id` (decimal string) | `found`, `events[]` | Pipeline trace when enabled |

Mutating RPCs (`ApplyConfig`, `ReloadFromFile`) leave the prior [runtime snapshot](/glossary/index.md#runtime-snapshot) unchanged when `ok` is false.

## OverlayApplyMode

Used by **`ApplyConfig`**. **`OVERLAY_APPLY_MODE_UNSPECIFIED` (0)** is treated as **merge**.

| Enum | Value | `conduitctl` equivalent | `overlay` field |
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
