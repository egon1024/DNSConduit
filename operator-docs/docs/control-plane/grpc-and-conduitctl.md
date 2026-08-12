# gRPC and conduitctl

This page is the **connection and command reference** for the optional [control plane](/glossary/index.md#control-plane): enabling gRPC, pointing **`conduitctl`** at a server, and invoking each subcommand. For **when** to reload, apply an [overlay](/glossary/index.md#overlay), or [export](/glossary/index.md#export), see [Reload and export](/control-plane/reload-and-export.md). For how config layers merge, see [Configuration model](/control-plane/configuration-model.md).

## Enabling the control plane

Conduit starts the gRPC listener only when the process **starts** with a `control:` block that sets **`listen_address`** (for example `127.0.0.1:5199`). Without it, DNS still runs but **`conduitctl apply`**, **`export`**, **`reload`**, **`trace`**, **`health`**, and live **`acl check`** are unavailable — use **SIGHUP** or a process restart to reload from disk instead. Offline **`validate --file`** and **`acl check --file`** still work.

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
| `conduitctl validate --file` | **No** | Offline YAML validation and runtime snapshot compile (Rhai, data sources, forward) |
| `conduitctl acl check` | Yes (default) | Dry-run client [ACL](/policy-routing/client-acls.md) for an IP against the **live** snapshot |
| `conduitctl acl check --file` | **No** | Same dry-run against a local config file (twin of `validate`) |
| `conduitctl trace` | Yes | Fetch pipeline trace events for a transaction id |
| `conduitctl health` | Yes | Per-backend health show, [freeze](/glossary/index.md#freeze), set up/down ([drain](/glossary/index.md#drain)), resume automatic |

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

Runs locally — no control plane connection. Validates YAML structure, then builds the same [runtime snapshot](/glossary/index.md#runtime-snapshot) Conduit uses at startup and reload: Rhai scripts are read and compiled, [data sources](/policy-routing/data-sources.md) are loaded, and forward settings are compiled. Paths resolve relative to the config file directory (or as absolute paths). Failures print prefixed errors (for example `script '…': …`, `rule '…': …`, `data source '…': …`) to stderr and exit non-zero.

The server also exposes **`ValidateConfig`** over gRPC for automation that already talks to the control plane; the CLI does not call it today.

### `acl check`

Dry-run client [ACL](/policy-routing/client-acls.md) evaluation for one IP. Prints **pretty JSON** to stdout. Exit code is **0** whenever the check itself succeeds (connect/load/evaluate); the JSON `decision` fields carry admit / drop / refuse / tag. The check is **read-only** — it does not bump ACL metrics or emit denial logs.

```bash
conduitctl acl check 203.0.113.50
conduitctl acl check 203.0.113.50 --listener public
conduitctl acl check 10.1.2.3 --file /path/to/conduit.yaml
conduitctl acl check 10.1.2.3 --file /path/to/conduit.yaml --listener public
```

| Mode | Flag | What is evaluated |
|------|------|-------------------|
| **Live** (default) | (none) | Running process snapshot + in-memory CIDR tables (includes overlay) via **`CheckAcl`** |
| **File** | `--file PATH` | Local compile of that YAML (same path resolution as `validate`) — no control plane |

Omit **`--listener`** to return one result object per listener. Filter with **`--listener NAME`** (resolved listener name, including the default `protocol:address` form). Unknown listener or invalid IP exits non-zero.

JSON shape:

```json
{
  "ip": "10.1.2.3",
  "source": "live",
  "results": [
    {
      "listener": "public",
      "decision": "admit",
      "matched": "corp_nets",
      "action": "accept"
    },
    {
      "listener": "internal",
      "decision": "tag",
      "tag": "corp",
      "matched": "corp_nets",
      "action": "tag"
    }
  ]
}
```

`source` is **`live`** or **`file`**. `decision` is `admit`, `drop`, `refuse`, or `tag` (tag name in sibling `tag`). `matched` is the CIDR view name, or **`default`** when `default_action` applied. `action` is the matched rule action, or `allow` / `deny` for the default.

### `trace`

```bash
conduitctl trace TXN_ID
```

Prints pipeline trace events when [tracing](/observability/tracing.md) captured the transaction. Exits non-zero if no trace was found.

### `health`

Per-[backend](/glossary/index.md#backend) health inspection and operator controls. Requires the control plane. Behavior: [Backend health](/policy-routing/backend-health.md). RPC reference: [Reference: gRPC and CLI — BackendHealth](/reference/grpc-and-cli.md#service-backendhealth).

```bash
conduitctl health show
conduitctl health show --pool default
conduitctl health show --pool default --backend resolver-a

conduitctl health freeze --global
conduitctl health freeze --pool default
conduitctl health freeze --pool default --backend resolver-a

conduitctl health set down --pool default --backend resolver-a
conduitctl health set up --pool default --backend resolver-a

conduitctl health resume --global
conduitctl health resume --pool default --backend resolver-a
```

| Subcommand | Purpose |
|------------|---------|
| `show` | Print observed/applied health, scope, eligibility, latency EWMA per backend |
| `freeze` | [Freeze](/glossary/index.md#freeze) — stop probe-driven changes to `applied` at the scope (`--global`, `--pool`, or `--pool` + `--backend`) |
| `set up\|down` | Manually set applied health and **imply [freeze](/glossary/index.md#freeze)** ([drain](/glossary/index.md#drain) = set down) |
| `resume` | **Resume automatic** — unfreeze and snap `applied` to `observed` |

`--backend` accepts the configured backend `name` or `host:port` address. Prefer **`health resume`** over ad-hoc clear sequences while [frozen](/glossary/index.md#freeze) — see [Clear-while-frozen](/policy-routing/backend-health.md#clear-while-frozen-footgun).

## Access logs

Every control RPC logs at **`info`** as **`control rpc`** (the **transport** line): gRPC method (`rpc`), peer address (`peer`), whether the connection used TLS (`tls`: `true`/`false` — transport encryption, distinct from requestor **`mtls`**), requestor identity (`requestor`: anonymous, API key, mTLS, or rejected), gRPC status (`grpc_code`, e.g. `Ok`, `InvalidArgument`), and latency (`latency_ms`). Request and response bodies are **not** logged.

Connections that never become an RPC — TCP accept errors, or TLS handshake failures (wrong protocol, bad/missing client certificate, etc.) — log at **`warn`** as **`control plane connection failed`** with **`tls`**, **`error`**, and **`peer`** when known.

Config RPCs (`ApplyConfig`, `ValidateConfig`, `ReloadFromFile`) additionally emit a **separate** `control rpc outcome` line (the **application** line) with `rpc`, `outcome` (`ok` or `rejected`), `error_count`, and the joined `errors`.

The two lines report different layers. A config that fails validation is rejected **in-band** — the RPC still succeeds at the transport layer — so it logs `control rpc` with `grpc_code=Ok` **and** a `control rpc outcome` with `outcome=rejected` and `error_count>0`. `conduitctl` surfaces the same rejection as a non-zero exit with the validation messages.

## gRPC reflection

When **`control.reflection_enabled: true`**, Conduit registers the standard gRPC **server reflection** service for dev and test tooling (for example discovering `ConduitControl` without a local copy of the proto). Leave reflection **off** in production unless you need it.

## Related topics

- [Reload and export](/control-plane/reload-and-export.md) — workflows, apply modes, export-before-clear
- [Configuration model](/control-plane/configuration-model.md) — merge rules and overlay scope
- [Config file](/control-plane/config-file.md) — startup path used by reload
- [Reference: gRPC and CLI](/reference/grpc-and-cli.md) — RPC and message reference
- [Glossary](/glossary/index.md) — [overlay](/glossary/index.md#overlay), [conduitctl](/glossary/index.md#conduitctl), [control plane](/glossary/index.md#control-plane)
