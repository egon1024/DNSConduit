# gRPC and conduitctl

This page documents the optional [control plane](/glossary/index.md#control-plane): the gRPC API Conduit exposes when `control.listen_address` is set at process start, and **`conduitctl`**, the operator CLI that calls it. For workflows (reload, [overlay](/glossary/index.md#overlay) apply modes, [export](/glossary/index.md#export)), see [Reload and export](/control-plane/reload-and-export.md). For how layers combine into [effective config](/glossary/index.md#effective-config), see [Configuration model](/control-plane/configuration-model.md).

## Overview

When the `control:` block is present at **process start**, Conduit listens for gRPC on `control.listen_address` (for example `127.0.0.1:5199`). The **`conduitctl`** binary connects to that address to apply overlays, export config, reload from disk, validate files offline, and fetch pipeline traces.

| Command | gRPC RPC | Needs running server? |
|---------|----------|------------------------|
| `conduitctl apply` | `ApplyConfig` | Yes |
| `conduitctl export` | `ExportConfig` | Yes |
| `conduitctl reload` | `ReloadFromFile` | Yes |
| `conduitctl validate --file` | (local only) | No |
| `conduitctl trace` | `GetTrace` | Yes |

Adding or changing `control:` via reload updates the stored config but does **not** start the gRPC listener today — **restart** the process after enabling control.

## CLI connection

Global flags apply to every subcommand:

| Flag / env | Default | Purpose |
|------------|---------|---------|
| `--endpoint` / `CONDUIT_CONTROL` | `http://127.0.0.1:5199` | gRPC control address |
| `--api-key` / `CONDUIT_API_KEY` | (none) | `Authorization: Bearer …` when the server requires API keys |

On success, mutating commands print `ok` to stdout. Failures exit non-zero with error text on stderr.

## `conduitctl apply`

Apply an [overlay](/glossary/index.md#overlay) patch to a running server. Modes match gRPC **`ApplyConfig`** and **`OverlayApplyMode`** (below).

```bash
conduitctl apply --file patch.yaml              # default: merge into overlay
conduitctl apply --merge --file patch.yaml      # explicit merge (same as default)
conduitctl apply --replace --file patch.yaml    # replace entire overlay
conduitctl apply --clear                        # clear overlay; no --file
```

| Flag | Conflicts with | Behavior |
|------|----------------|----------|
| (default) | — | **Merge** patch into accumulated overlay |
| `--merge` | `--replace`, `--clear` | Explicit **merge** (same as default) |
| `--replace` | `--merge`, `--clear` | **Replace** overlay with patch; `schema_version`-only patch **clears** overlay |
| `--clear` | `--merge`, `--replace`, `--file` | **Clear** overlay without re-reading startup file |

**`--file`** is required for merge and replace; omit it for **`--clear`**.

Operator workflows and examples: [Reload and export — apply modes](/control-plane/reload-and-export.md#apply-modes).

## Other commands

```bash
conduitctl export [--output PATH]   # effective YAML; default stdout (-)
conduitctl reload                   # reload from disk; clears overlay
conduitctl validate --file PATH     # offline validation; no server required
conduitctl trace TXN_ID             # pipeline trace for a transaction id
```

## ApplyConfig and OverlayApplyMode

**`ApplyConfig`** applies an overlay patch using **`OverlayApplyMode`**. The protobuf enum values map to CLI flags as follows:

| `OverlayApplyMode` (proto) | Value | CLI equivalent | `overlay` field |
|----------------------------|-------|----------------|-----------------|
| `OVERLAY_APPLY_MODE_UNSPECIFIED` | 0 | (default) **merge** | Patch required |
| `OVERLAY_APPLY_MODE_MERGE` | 1 | default or `--merge` | Patch required |
| `OVERLAY_APPLY_MODE_REPLACE` | 2 | `--replace` | Patch required; empty patch clears overlay |
| `OVERLAY_APPLY_MODE_CLEAR` | 3 | `--clear` | Omit / unset |

Request shape (conceptual):

```protobuf
message ApplyConfigRequest {
  Config overlay = 1;           // patch for MERGE/REPLACE
  OverlayApplyMode mode = 2;
}
```

**Merge:** combine `overlay` with the active accumulated overlay, then merge file + overlay into [effective config](/glossary/index.md#effective-config).

**Replace:** set overlay to the patch; if the patch sets no overlay-eligible fields (`schema_version` only), clear the overlay.

**Clear:** drop the overlay; do not re-read the startup config file.

Response: `ok` plus `errors` when validation fails (prior [runtime snapshot](/glossary/index.md#runtime-snapshot) unchanged).

Other RPCs (summary):

| RPC | Purpose |
|-----|---------|
| `GetConfig` | Return current effective config |
| `ExportConfig` | Serialize effective config (`format`: `yaml` or `json`) |
| `ReloadFromFile` | [Reload from disk](/glossary/index.md#reload-from-disk) — re-read startup file, clear overlay |
| `ValidateConfig` | Validate a config message without applying |
| `Health` | Liveness (`status`: `serving`) |
| `GetTrace` | Pipeline trace events for a transaction id |

Field-exact RPC and message reference: [Reference: gRPC and CLI](/reference/grpc-and-cli.md) (when published).

## Related topics

- [Reload and export](/control-plane/reload-and-export.md) — SIGHUP, reload, apply modes, export-before-clear
- [Configuration model](/control-plane/configuration-model.md) — file layer, overlay merge rules
- [Config file](/control-plane/config-file.md) — startup path used by reload
- [Glossary](/glossary/index.md) — [overlay](/glossary/index.md#overlay), [clear overlay without reload](/glossary/index.md#clear-overlay-without-reload), [conduitctl](/glossary/index.md#conduitctl)
