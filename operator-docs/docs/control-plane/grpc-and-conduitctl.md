# gRPC and conduitctl

This page is the **connection and command reference** for the optional [control plane](/glossary/index.md#control-plane): enabling gRPC, pointing **`conduitctl`** at a server, and invoking each subcommand. For **when** to reload, apply an [overlay](/glossary/index.md#overlay), or [export](/glossary/index.md#export), see [Reload and export](/control-plane/reload-and-export.md). For how config layers merge, see [Configuration model](/control-plane/configuration-model.md).

## Enabling the control plane

Conduit starts the gRPC listener only when the process **starts** with a `control:` block that sets **`listen_address`** (for example `127.0.0.1:5199`). Without it, DNS still runs but **`conduitctl apply`**, **`export`**, **`reload`**, and **`trace`** are unavailable — use **SIGHUP** or a process restart to reload from disk instead.

Adding or changing `control:` via reload updates the stored config but does **not** start or rebind the listener today. **Restart** `conduit` after enabling or moving the control address.

Config fields: [Reference: control](/reference/config-schema/control.md).

## Connecting

Global flags apply to every subcommand:

| Flag / env {: .column-no-wrap } | Default {: .column-no-wrap } | Purpose |
|------------|---------|---------|
| `--endpoint` / `CONDUIT_CONTROL` | `http://127.0.0.1:5199` | Control plane URL |
| `--api-key` / `CONDUIT_API_KEY` | (none) | `Authorization: Bearer …` when the server requires API keys |

Use **`http://`** when the listener is plain TCP. When **`control.tls`** is configured, use **`https://`** (or the scheme your TLS setup expects) on the same host and port.

On success, mutating commands print `ok` to stdout. Failures exit non-zero with error text on stderr.

## Authentication

| Server config | Client requirement |
|---------------|-------------------|
| `control.api_keys` **empty** | No credentials (anonymous access to control RPCs) |
| `control.api_keys` **non-empty** | Valid key via **`Authorization: Bearer …`** or header **`x-api-key`** — `conduitctl` uses Bearer (`--api-key` / `CONDUIT_API_KEY`) |
| `control.tls.client_ca_path` set | Server requires a client certificate (mTLS) in addition to any API key rules |

Details: [API keys](/security/api-keys.md), [mTLS](/security/mtls.md).

## Commands

| Command | Needs server? | Purpose |
|---------|---------------|---------|
| `conduitctl apply` | Yes | Patch the in-memory overlay ([apply modes](/control-plane/reload-and-export.md#apply-modes)) |
| `conduitctl export` | Yes | Print effective config as YAML |
| `conduitctl reload` | Yes | [Reload from disk](/glossary/index.md#reload-from-disk); clear overlay |
| `conduitctl validate --file` | **No** | Offline structural validation of any YAML path |
| `conduitctl trace` | Yes | Fetch pipeline trace events for a transaction id |

RPC methods and messages: [Reference: gRPC and CLI](/reference/grpc-and-cli.md).

### `apply`

```bash
conduitctl apply --file patch.yaml              # default: merge into overlay
conduitctl apply --merge --file patch.yaml      # explicit merge
conduitctl apply --replace --file patch.yaml    # replace entire overlay
conduitctl apply --clear                        # clear overlay; no --file
```

| Flag | Conflicts with | Behavior |
|------|----------------|----------|
| (default) | — | **Merge** patch into accumulated overlay |
| `--merge` | `--replace`, `--clear` | Explicit merge (same as default) |
| `--replace` | `--merge`, `--clear` | Replace overlay; `schema_version`-only patch **clears** overlay |
| `--clear` | `--merge`, `--replace`, `--file` | Clear overlay without re-reading startup file |

**`--file`** is required for merge and replace; omit it for **`--clear`**.

Patch files are **sparse YAML** — only keys you include are sent. Overlays **must not** include **`rules:`**, **`metrics:`**, or **`tracing:`**; apply is rejected if those sections are present. Semantics, examples, and overlay scope: [Reload and export — apply modes](/control-plane/reload-and-export.md#apply-modes).

### `export`

```bash
conduitctl export                    # stdout
conduitctl export --output PATH      # write file
```

Returns effective config as YAML (file layer + overlay, defaults normalized). See [Reload and export — export](/control-plane/reload-and-export.md#export-effective-configuration).

### `reload`

```bash
conduitctl reload
```

Re-reads the config path from process startup and clears the overlay. Same semantics as **SIGHUP** when gRPC is enabled. See [Reload from disk](/control-plane/reload-and-export.md#reload-from-disk-sighup-and-conduitctl-reload).

### `validate`

```bash
conduitctl validate --file PATH
```

Runs locally — no control plane connection. Validates structure only; does not check Rhai script paths, CSV files, or TLS PEM paths on disk. The server also exposes **`ValidateConfig`** over gRPC for automation that already talks to the control plane; the CLI does not call it today.

### `trace`

```bash
conduitctl trace TXN_ID
```

Prints pipeline trace events when [tracing](/observability/tracing.md) captured the transaction. Exits non-zero if no trace was found.

## Access logs

Successful and failed control RPCs log at **`info`** as **`control rpc`**: gRPC method, peer address, requestor identity (anonymous, API key, mTLS, or rejected), status, and latency. Request and response bodies are **not** logged.

## gRPC reflection

When **`control.reflection_enabled: true`**, Conduit registers the standard gRPC **server reflection** service for dev and test tooling (for example discovering `ConduitControl` without a local copy of the proto). Leave reflection **off** in production unless you need it.

## Related topics

- [Reload and export](/control-plane/reload-and-export.md) — workflows, apply modes, export-before-clear
- [Configuration model](/control-plane/configuration-model.md) — merge rules and overlay scope
- [Config file](/control-plane/config-file.md) — startup path used by reload
- [Reference: gRPC and CLI](/reference/grpc-and-cli.md) — RPC and message reference
- [Glossary](/glossary/index.md) — [overlay](/glossary/index.md#overlay), [conduitctl](/glossary/index.md#conduitctl), [control plane](/glossary/index.md#control-plane)
