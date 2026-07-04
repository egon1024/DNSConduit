# Config schema: health

Field reference for the optional `health:` block on a [pool](/glossary/index.md#pool) and per-[backend](/glossary/index.md#backend) probe overrides. For behavior — probes, passive fast-trip, Route eligibility, operator controls — see [Backend health](/policy-routing/backend-health.md).

## Location

```yaml
pools:
  - name: default
    health:          # pool-level health settings
      enabled: true
      ...
    backends:
      - address: "10.0.0.1:53"
        probe_qname: "probe.example."   # optional per-backend override
```

When `health` is absent or `enabled` is false, Conduit does not run health checks for that pool — selection stays weight-based only.

## Pool `health` object

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `enabled` | boolean | no | `false` | When `true`, start active probing and health-aware routing for this pool. |
| `interval_ms` | integer | no | **1000** | Target time between probe attempts per backend (milliseconds). Minimum **100**. |
| `timeout_ms` | integer | no | same as `interval_ms` | Probe reply timeout (milliseconds). Must be **≥ 1** when set. |
| `rise` | integer | no | **3** | Consecutive successful probes required to mark a backend **up**. Must be **≥ 1**. |
| `fall` | integer | no | **2** | Consecutive failed probes required to mark a backend **down**. Must be **≥ 1**. |
| `probe_qname` | string | no | **`.`** | DNS name sent in probe queries (pool template). |
| `probe_qtype` | string | no | **NS** | Query type for probes (for example `A`, `AAAA`, `NS`). |
| `acceptable_rcodes` | list of strings | no | (any well-formed response) | When set, only listed [RCODE](/glossary/index.md#rcode) names count as probe success (for example `NOERROR`, `NXDOMAIN`). Empty list means any well-formed DNS response proves liveness. |
| `initial_state` | string | no | **`optimistic`** | Eligibility policy for **new** backends — see [Initial state](#initial-state). |
| `latency_weighting` | boolean | no | `false` | When `true`, scale effective weight by probe latency [EWMA](/glossary/index.md#ewma) among eligible backends. |
| `min_eligible` | integer | no | **0** | Fail-open floor — when eligible count in the pool is below this value, Route ignores health and treats all backends as eligible. Must be **≥ 0**. |
| `passive_fast_trip` | boolean | no | `true` | When `true`, live forward timeouts/errors can mark a backend down before probe `fall` completes. |
| `passive_fall` | integer | no | **2** | Consecutive passive (forward) failures required to mark a backend **down**. Must be **≥ 1** when set. |

Internal constants (not YAML keys): latency EWMA **alpha = 0.2**; latency weight **floor = 0.25** relative to the pool's fastest EWMA.

### Initial state

| Value | Behavior for a **new** backend |
|-------|-------------------------------|
| `optimistic` | Eligible immediately until probes or passive fast-trip prove otherwise (default). |
| `require_1_good` | Not eligible until one successful probe. |
| `require_full_rise` | Not eligible until `rise` consecutive successful probes. |

At process start, the fail-open floor covers pools where every backend is still unknown.

## Per-backend overrides

Nested under `pools[].backends[]` when health is enabled for the pool:

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `probe_qname` | string | no | pool `probe_qname` | Override probe query name for this backend only. |
| `probe_qtype` | string | no | pool `probe_qtype` | Override probe query type. |
| `probe_source` | string | no | (system bind) | Local IPv4 or IPv6 address to bind for probes to this backend. |
| `transport` | string | no | — | **Reserved / forward-compatible only.** Not honored when it diverges from [`forward.upstream_transport`](/reference/config-schema/forward.md). Probes always use the global forward transport. |

The `backend` label on [health metrics](/observability/built-in-metrics.md#backend-health) uses the configured `name` when set, otherwise `address`.

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| `interval_ms` ≥ 100 | `health.interval_ms … is below the 100ms floor` |
| `timeout_ms` ≥ 1 when set | `health.timeout_ms must be >= 1 when set` |
| `rise` / `fall` ≥ 1 | `health.rise must be >= 1` / `health.fall must be >= 1` |
| `passive_fall` ≥ 1 when set | `health.passive_fall must be >= 1 when set` |
| Valid `probe_qtype` | `health.probe_qtype: …` |
| Valid `acceptable_rcodes` names | `health.acceptable_rcodes: …` |
| Valid `initial_state` | `health.initial_state '…' must be optimistic, require_1_good, or require_full_rise` |
| Valid `probe_source` address | per-backend parse errors |

Validate with `conduitctl validate --file …` or at load time.

## Reload behavior

- **Hot:** probe configuration fields reload with the [runtime snapshot](/glossary/index.md#runtime-snapshot).
- **Preserved:** health state for unchanged backends (same address and probe semantics).
- **Reset:** new backend, address change, or probe-semantics change.

See [Backend health — Reload and health state](/policy-routing/backend-health.md#reload-and-health-state).

## Example

```yaml
pools:
  - name: default
    health:
      enabled: true
      interval_ms: 1000
      timeout_ms: 500
      rise: 3
      fall: 2
      probe_qname: "health.example."
      probe_qtype: A
      acceptable_rcodes:
        - NOERROR
        - NXDOMAIN
      initial_state: optimistic
      latency_weighting: true
      min_eligible: 1
      passive_fast_trip: true
      passive_fall: 2
    backends:
      - address: "10.0.0.1:53"
        name: resolver-a
        weight: 70
      - address: "10.0.0.2:53"
        name: resolver-b
        weight: 30
        probe_qname: "resolver-b.health.example."
```

## Related topics

- [Backend health](/policy-routing/backend-health.md) — mental model and operations
- [Reference: pools](/reference/config-schema/pools.md) — pool and backend objects
- [Reference: forward](/reference/config-schema/forward.md) — `upstream_transport` (probe transport matches this)
- [Built-in metrics — Backend health](/observability/built-in-metrics.md#backend-health)
