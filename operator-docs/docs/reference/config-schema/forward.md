# Config schema: forward

This page lists the fields for the top-level **`forward:`** block — upstream transport, timeouts, concurrency limits, and global egress source addresses. For behavior — how Conduit binds when forwarding, transport fallback, and rule overrides — see [Architecture and packet path — Forward](/concepts/architecture-and-packet-path.md#forward) and [Dual-stack forwarding](/guides/dual-stack-forwarding.md).

Per-pool egress overrides are described in [Reference: pools](/reference/config-schema/pools.md) (`sources_v4` / `sources_v6`).

## `forward`

| Property | Value |
|----------|--------|
| **Type** | Mapping (object) |
| **Required** | No — defaults apply when the block is omitted |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

When **`forward:`** is omitted, Conduit applies the same defaults at parse time as a sparse export (see [Defaults when omitted](#defaults-when-omitted)).

## Block fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `timeout_ms` | integer | no | **2000** | Per-attempt upstream wait (milliseconds). Socket read/write timeouts for forward use the same value. See [Architecture — Forward](/concepts/architecture-and-packet-path.md#forward). |
| `outstanding_per_backend` | integer | no | **100** | Maximum in-flight forward operations per upstream [backend](/glossary/index.md#backend) `address`. When exceeded, new forwards to that backend fail with `reason="table_full"` on [`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total). |
| `sources_v4` | list of strings | no | `[]` | Global IPv4 addresses Conduit may bind for upstream egress when forwarding to IPv4 backends. Empty list → OS default bind behavior. Per-pool lists override when non-empty — [Reference: pools](/reference/config-schema/pools.md). |
| `sources_v6` | list of strings | no | `[]` | Global IPv6 egress sources for IPv6 backends. Same override rules as `sources_v4`. |
| `source_selection` | string | no | **`round_robin`** | How Conduit picks among allowed sources at [Forward](/concepts/architecture-and-packet-path.md#forward). Only **`round_robin`** is supported today. Empty string in YAML → **`round_robin`**. |
| `upstream_transport` | string | no | **`udp_only`** | Upstream protocol policy. Empty string in YAML → **`udp_only`**. See [Upstream transport](#upstream-transport). |
| `client_tcp_uses_upstream_tcp` | boolean | no | **`false`** | When **`true`**, a client query received over TCP may use upstream TCP when `upstream_transport` allows TCP. |

### Defaults when omitted

| Field | Effective value when `forward:` omitted |
|-------|----------------------------------------|
| `timeout_ms` | **2000** |
| `outstanding_per_backend` | **100** |
| `sources_v4` / `sources_v6` | empty lists |
| `source_selection` | **`round_robin`** |
| `upstream_transport` | **`udp_only`** |
| `client_tcp_uses_upstream_tcp` | **`false`** |

See also [Minimal configuration — Defaults](/getting-started/minimal-configuration.md#defaults-you-do-not-need-to-write-yet).

### Upstream transport

| Value | Behavior |
|-------|----------|
| **`udp_only`** | Upstream queries use UDP only (default). |
| **`tcp_only`** | Upstream queries use TCP only. |
| **`prefer_udp_with_tcp_fallback`** | UDP first; if the UDP response has the **TC** (truncated) bit set, Conduit retries that attempt over TCP. |

Invalid values fail validation at load time.

Rule and [Rhai](/rhai/index.md) **`set_source_*`** overrides must still be in the allowed source set for the selected [pool](/glossary/index.md#pool) — see [Dual-stack forwarding](/guides/dual-stack-forwarding.md#choosing-an-egress-source).

### `sources_v4` / `sources_v6` constraints

Same rules as pool-level source lists:

- Each entry must be a valid IPv4 or IPv6 **address** (not `host:port`).
- Entries must not be empty strings.
- At most **32** addresses per list.

Errors use the `forward.sources_v4[…]` / `forward.sources_v6[…]` prefix when the list is under **`forward:`**.

## Reload and restart

| Change | Stored in new snapshot? | On-the-wire effect |
|--------|-------------------------|--------------------|
| `timeout_ms`, `outstanding_per_backend`, `sources_*`, transport fields | Yes — stored in new snapshot | **Restart required** — egress sockets and forward runtime state bind at process start |
| Overlay patch including **`forward:`** | Replaces entire file-layer **`forward:`** section when present | Same — restart to apply on wire |

After a successful reload that changes **`forward`**, Conduit logs **`forward egress: pending (restart required)`** and continues with the **previous** egress behavior until you restart. See [Configuration model — Pending reconcile](/control-plane/configuration-model.md#pending-reconcile-restart-required).

Changes to **`forward:`** via **`conduitctl apply`** are overlay-eligible (whole-section replace when the patch includes **`forward:`**).

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| `source_selection` not **`round_robin`** when non-empty | `forward.source_selection '…' must be round_robin (slice A)` |
| Invalid `upstream_transport` | `forward.upstream_transport '…' must be udp_only, tcp_only, or prefer_udp_with_tcp_fallback` |
| Invalid / empty `sources_v4` / `sources_v6` entry | `forward.sources_v4[…] …` / `forward.sources_v6[…] …` |
| More than **32** entries per source list | `sources_v4 has N entries; maximum is 32` (same for v6) |

Validate with `conduitctl validate --file …`. Validation does not probe upstream reachability.

## Example configuration

```yaml
forward:
  timeout_ms: 3000
  outstanding_per_backend: 200
  sources_v4:
    - "10.0.0.10"
  sources_v6:
    - "2001:db8::10"
  source_selection: round_robin
  upstream_transport: prefer_udp_with_tcp_fallback
  client_tcp_uses_upstream_tcp: true
```

## Related topics

- [Dual-stack forwarding](/guides/dual-stack-forwarding.md) — global vs per-pool sources, rules, Rhai
- [Architecture and packet path — Forward](/concepts/architecture-and-packet-path.md#forward) — timeout, transport, errors
- [Retries and transactions](/policy-routing/retries-and-transactions.md) — retry after upstream timeout
- [Reference: pools](/reference/config-schema/pools.md) — per-pool `sources_v4` / `sources_v6`
- [Reference: listeners](/reference/config-schema/listeners.md) — client ingress vs upstream transport
- [Troubleshooting — Upstream timeouts](/troubleshooting/index.md#upstream-timeouts-and-slow-responses)
- [Config schema overview](/reference/config-schema/index.md)
