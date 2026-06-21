# Config schema: listeners

Field reference for the top-level **`listeners:`** block — [dataplane](/glossary/index.md#dataplane) **ingress** where clients send DNS queries. For the query path after a packet arrives, see [Architecture and packet path — Receive](/concepts/architecture-and-packet-path.md#receive). For a minimal runnable example, see [Minimal configuration](/getting-started/minimal-configuration.md).

DNS listeners are separate from the optional gRPC **`control:`** listener — see [Security](/security/index.md) and [Reference: control](/reference/config-schema/control.md).

## `listeners`

| Property | Value |
|----------|--------|
| **Type** | Mapping (object) |
| **Required** | Yes for a runnable installation (at least one entry under `listeners.listeners`) |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

When the block is omitted entirely, Conduit applies defaults at parse time (`threads: 1`, `reuse_port: false`, empty socket buffer overrides, no bound addresses). An empty `listeners.listeners` list may pass validation but Conduit will not accept client DNS — see [What makes a config runnable](/control-plane/config-file.md#what-makes-a-config-runnable).

## Block fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `threads` | integer | no | **1** | Number of worker threads **per** [listener entry](#listener-object) below. Must be **≥ 1**. Each worker runs the full query [pipeline](/concepts/architecture-and-packet-path.md#pipeline-phases) on its thread in current **`sync`** builds. |
| `reuse_port` | boolean | no | **`false`** | When **`true`**, UDP sockets use **`SO_REUSEPORT`** (Unix only) so multiple workers can bind the same address. Use **`true`** when `threads` > **1** on UDP. Ignored on non-Unix platforms and for TCP listeners. |
| `rcvbuf` | integer | no | **0** (OS default) | When **> 0**, sets the UDP socket receive buffer size (bytes) before bind. **0** leaves the OS default. Applies to UDP listeners only. |
| `sndbuf` | integer | no | **0** | Reserved — accepted in YAML but **not applied** to sockets in current releases. |
| `listeners` | list | yes (for DNS service) | `[]` | One or more [listener objects](#listener-object). Conduit binds each entry at **process start**. |

### Worker count

Total ingress workers = **`threads` ×** number of entries in `listeners.listeners`.

Example — two UDP sockets and `threads: 2` starts **four** worker threads (two per address). Under the **`sync`** runtime, each worker handles one query at a time through upstream wait; see [Architecture and packet path — Concurrency and workers](/concepts/architecture-and-packet-path.md#concurrency-and-workers).

### `reuse_port` and `threads`

| Client protocol | `threads` | Recommended `reuse_port` |
|-----------------|-----------|----------------------------|
| **UDP** | **1** | **`false`** (default) |
| **UDP** | **> 1** | **`true`** — required on Unix so every worker can bind the same address |
| **TCP** | any | N/A — TCP uses **`SO_REUSEADDR`** internally; `reuse_port` is ignored |

Without **`reuse_port: true`**, a second UDP worker binding the same address typically fails at process start with a bind error.

## Listener object

Each list entry under `listeners.listeners` is one bind address and protocol.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `address` | string | yes | — | Client-facing socket address as `host:port`. IPv6 literals use bracket notation, for example **`[::1]:53`**. Must parse as a socket address; must not be empty. |
| `protocol` | string | yes | — | **`udp`** or **`tcp`**. Comparison is case-insensitive. Values other than **`tcp`** are treated as UDP. |

### Address format

| Form | Example | Notes |
|------|---------|--------|
| IPv4 loopback | `"127.0.0.1:15353"` | Common in lab configs (high port avoids mDNS on **5353**) |
| All interfaces | `"0.0.0.0:53"` | Requires privilege to bind port **53** on many systems |
| IPv6 | `"[::1]:15353"` | Brackets required around the literal |

The string in `address` is also the **`listener`** label on built-in [metrics](/observability/metrics.md) such as [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) (together with **`protocol`**: `udp` or `tcp`).

### UDP vs TCP

| | **UDP** | **TCP** |
|---|---------|---------|
| Wire format | One datagram per query | RFC 1035 length-prefixed messages per connection |
| Typical use | Resolver traffic, high volume | Clients that require TCP (large responses, `+tcp` in `dig`) |
| Socket tuning | `reuse_port`, `rcvbuf` apply | `reuse_port` and `rcvbuf` ignored |
| Upstream transport | Controlled by **`forward.upstream_transport`** | When **`forward.client_tcp_uses_upstream_tcp`** is **`true`**, TCP client queries can use upstream TCP |

Conduit does not terminate DNS-over-TLS or DNS-over-HTTPS on these listeners — only plain UDP and TCP DNS.

## Reload and restart

Listener sockets are opened at **process start** from the config present when `conduit` starts.

| Change | Snapshot after reload/apply? | On-the-wire effect |
|--------|----------------------------|--------------------|
| `pools`, `rules`, `orchestrator`, … | Yes — hot for new queries | N/A (not listener fields) |
| **`listeners`** (addresses, `threads`, `reuse_port`, `rcvbuf`, entries) | Yes — stored in new snapshot | **Restart required** — existing sockets keep serving until restart |

After a successful reload that changes **`listeners`**, Conduit logs **`pending (restart required)`** and continues on the **previous** bind until you restart the process. See [Configuration model — Pending reconcile](/control-plane/configuration-model.md#pending-reconcile-restart-required) and [Reload and export](/control-plane/reload-and-export.md).

`conduitctl validate` does **not** bind sockets — bind failures (address in use, permission denied) appear at **startup** or after **restart**, not during validate.

## Validation summary

| Rule | Error or outcome if violated |
|------|------------------------------|
| `listeners.threads` ≥ **1** | `listeners.threads must be >= 1` |
| Listener `address` non-empty | `listener address must not be empty` |
| `address` parses as socket address | Bind error at startup (not caught by validate) |
| Duplicate bind without `reuse_port` | Bind error at startup when `threads` > **1** or duplicate entries contend |

Validate with `conduitctl validate --file …` or load via the running process; see [Config file](/control-plane/config-file.md).

## Example configuration

Lab UDP listener with two workers (matches common test fixtures):

```yaml
listeners:
  threads: 2
  reuse_port: true
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
```

Dual-stack ingress — separate entries per address family:

```yaml
listeners:
  threads: 1
  listeners:
    - address: "0.0.0.0:53"
      protocol: udp
    - address: "[::]:53"
      protocol: udp
    - address: "0.0.0.0:53"
      protocol: tcp
```

Optional receive buffer tuning for high-volume UDP:

```yaml
listeners:
  threads: 4
  reuse_port: true
  rcvbuf: 4194304   # 4 MiB — only applied when > 0
  listeners:
    - address: "10.0.0.5:53"
      protocol: udp
```

## Related topics

- [Minimal configuration](/getting-started/minimal-configuration.md) — smallest `listeners` + `pools` example
- [First query](/getting-started/first-query.md) — test with `dig` after bind
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — Receive phase and worker concurrency
- [Configuration model](/control-plane/configuration-model.md) — snapshot updates and restart-required changes
- [Built-in metrics](/observability/built-in-metrics.md) — `listener` and `protocol` labels
- [Security](/security/index.md) — dataplane listeners vs control-plane gRPC
- [Config schema overview](/reference/config-schema/index.md)
