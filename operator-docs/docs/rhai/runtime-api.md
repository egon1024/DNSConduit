---
toc_depth: 3
toc_collapsible: true
---

# Runtime API (`runtime`)

The **`runtime`** scope object exposes **read-only process state** at hook entry. Today the only domain is **`routing`** — pool-wide health and in-flight counts aligned with [Route](/concepts/architecture-and-packet-path.md#route).

For why this is separate from **`txn`**, see [Host API overview](/rhai/host-api.md#five-scope-objects).

## Availability

| Piece | When present |
|-------|----------------|
| **`runtime`** in scope | Every Rule Rhai hook run |
| Meaningful **`runtime.routing()`** data | Pool has **`health.enabled: true`** in config |

When health is **disabled** for a pool, views still work: **`configured()`** is `true` and counts reflect **health-off** semantics (`eligible_count` equals `configured_count`; per-backend **`applied`** is `"up"`). **Unknown** pool or backend names return empty views (`configured: false`).

## When values are taken { #when-values-are-taken }

When your script runs on the [request hook](/rhai/hooks-and-phases.md#request-hook) or [response hook](/rhai/hooks-and-phases.md#response-hook), Conduit captures **one** routing and health snapshot at the **start of that hook phase** — before your first line of Rhai runs. The snapshot includes backend health (`applied`, `observed`, eligibility), [EWMA](/glossary/index.md#ewma), in-flight forward counts, and fail-open flags as they were **at that moment**.

It is **not** a view from process startup. Each `runtime.routing().pool(...)` or `.backend(...)` call in the **same** script reads that **same** snapshot — calls do not re-query live health.

| What to expect | Practical meaning |
|----------------|-------------------|
| **Captured at hook phase start** | Request rules or response rules **begin** for this query on this worker; health and routing fields reflect state then. |
| **Frozen while the script runs** | Probes, **`conduitctl health`** drain/freeze, and [config reload](/control-plane/reload-and-export.md) can change backends after the snapshot was taken. Your variables do not update until the next hook. |
| **Snapshot build vs reads** | Building the snapshot walks configured pools/backends once at hook entry (bounded by config size, outside Rhai **`max_operations`**). Each **`runtime.routing().pool()`** / **`.backend()`** call inside the script is an O(1) read and **does** count toward **`max_operations`** — see [Sandbox limits — Host API and `max_operations`](/rhai/sandbox-limits.md#host-api-and-max_operations). |
| **New snapshot each hook** | Request hook, each response hook, and each [retry](/policy-routing/retries-and-transactions.md) response pass builds a fresh snapshot. Do not carry values from one hook to the next. |
| **Close to the next Route pick** | [Route](/concepts/architecture-and-packet-path.md#route) uses the same health sources when it selects the next backend on this query (Route reads again at selection time, so health can move in the short gap after your script). |

**`runtime.config_generation()`** (and **`txn.config_generation()`**) tell you which [config generation](/control-plane/configuration-model.md) the query is running under — useful after reload to gate new policy, not for measuring probe latency or sub-second health flaps.

Use **`runtime`** to **branch policy** (failover pool, retry on applied down). For live operational view, use [metrics](/observability/metrics.md) and **`conduitctl health`** — not Rhai reads on every query.

## Query by name { #query-by-name }

`runtime.routing()` answers about pools and backends you **name explicitly**. Pass a pool name or backend id; Conduit returns health and routing for that target.

| You call | You pass | You get |
|----------|----------|---------|
| **`runtime.routing().pool(name)`** | pool name (for example `"primary"`) | pool-wide counts and flags (eligible backends, fail-open, …) |
| **`runtime.routing().backend(pool, id)`** | pool + backend id | health and routing for that backend |
| **`runtime.routing().backend_for_attempt(...)`** | **`txn.selected_pool()`** and **`txn.selected_backend_name()`** | same, for the forward attempt that just finished |

**Where to get names:**

- **Config and rules** — literal pool names in your script (`"primary"`, `"secondary"`).
- **This query** — **`txn.selected_pool()`** / **`txn.selected_backend_name()`** on the response hook.
- **`lookup()` tables** — resolve qname or other keys to a pool name in **`data_sources:`**, then pass that string to **`runtime.routing().pool(...)`**.
- **Several pools** — name each pool in the script, or resolve names from a lookup table.

---

## `runtime`

Top-level read-only methods on the **`runtime`** scope object.

<p class="txn-api-index" markdown="1">

**Methods:** [`runtime.config_generation()`](#runtimeconfig_generation) · [`runtime.routing()`](#runtimerouting)

</p>

<div class="txn-api-entry" markdown="1">

### `runtime.config_generation()` {#runtimeconfig_generation}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64`

[Snapshot generation](/control-plane/configuration-model.md) captured when this hook run started.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Behavior

- Same generation as [`txn.config_generation()`](/rhai/txn-api.md#txnconfig_generation) on this query — both reflect the active config snapshot at hook entry.
- Matches [`conduit_config_generation`](/observability/built-in-metrics.md#conduit_config_generation) for that snapshot.
- Use after reload to gate canary policy (for example only apply new logic when generation ≥ N).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `runtime.routing()` {#runtimerouting}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `RoutingRuntime`

Routing and health snapshot for configured pools/backends.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Behavior

- **Read-only** — does not change routing, health, or transaction state.
- Built once when **this hook phase begins** from the health side-table, pool config, and outstanding-forward counts; every `runtime.routing()` call in the same script shares it.
- Per-call reads count toward [sandbox `max_operations`](/rhai/sandbox-limits.md#host-api-and-max_operations); snapshot build at hook entry does not.

</div>

</div>

---

## `RoutingRuntime`

Returned by **`runtime.routing()`**. Pass a pool or backend **name** you already have — see [Query by name](#query-by-name).

<p class="txn-api-index" markdown="1">

**Methods:** [`pool(name)`](#routingruntimepoolname) · [`backend(pool, id)`](#routingruntimebackendpool-id) · [`backend_for_attempt(pool, backend_id)`](#routingruntimebackend_for_attemptpool-backend_id)

</p>

<div class="txn-api-entry" markdown="1">

### `pool(name)` {#routingruntimepoolname}

<div class="txn-api-brief" markdown="1">

`RoutingRuntime` method · `name`: string · returns `PoolRuntime`

Summary for the whole pool — eligible backend count, fail-open, in-flight totals, and similar.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Behavior

- **`name`** must match a **`pools[].name`** in config for **`configured() == true`**.
- Unknown pool: empty view (`configured: false`, counts `0`).
- Does not select a pool for this query — use **`txn.set_pool`** for policy writes.
- Returns **`PoolRuntime`** — see [Pool view](#pool-view-poolruntime) for field methods (`eligible_count`, `fail_open_active`, …).

#### Example

Request-hook failover when not all backends are eligible (repository fixture `routing-pool-failover.rhai`):

```rhai
let primary = runtime.routing().pool("primary");
if primary.configured() && primary.eligible_count() < primary.configured_count() {
    txn.set_pool("secondary");
}
```

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `backend(pool, id)` {#routingruntimebackendpool-id}

<div class="txn-api-brief" markdown="1">

`RoutingRuntime` method · `pool`, `id`: strings · returns `BackendRuntime`

Per-backend health/routing view at hook entry.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Behavior

- **`pool`** — pool name; **`id`** — backend **`name`** when set and unique in the pool, otherwise **`ip:port`** (same rules as metrics labels and **`conduitctl health`**).
- Unknown pool/backend pair: empty view (`configured: false`).
- Use when you know the pool and backend identity from config or **`lookup()`** — on the response hook prefer **`backend_for_attempt`** with **`txn.selected_*`** for the attempt that just completed.
- Returns **`BackendRuntime`** — see [Backend view](#backend-view-backendruntime) for field methods (`applied`, `eligible`, …).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `backend_for_attempt(pool, backend_id)` {#routingruntimebackend_for_attemptpool-backend_id}

<div class="txn-api-brief" markdown="1">

`RoutingRuntime` method · two strings · returns `BackendRuntime`

View for a specific pool/backend pair — use with the current attempt context.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

Primarily [response hook](/rhai/hooks-and-phases.md#response-hook) — also available on the request hook when **`txn.selected_pool()`** / **`txn.selected_backend_name()`** are already set.

#### Behavior

- Pass **`txn.selected_pool()`** and **`txn.selected_backend_name()`** for the forward attempt that just completed.
- Empty **`pool`** or **`backend_id`** returns an empty backend view (`configured: false`).
- Same field semantics as **`backend(pool, id)`** — convenience wrapper for response-hook retry and metrics branching.

#### Example

Response hook — retry when the attempt backend is applied down (repository fixture `routing-backend-attempt.rhai`):

```rhai
let backend = runtime.routing().backend_for_attempt(
    txn.selected_pool(),
    txn.selected_backend_name()
);
if backend.configured() && backend.applied() == "down" {
    txn.request_retry();
}
```

</div>

</div>

---

## Pool view (`PoolRuntime`) { #pool-view-poolruntime }

Returned by **`runtime.routing().pool("pool_name")`**. Methods below return pool-wide counts and flags such as **`eligible_count()`** and **`fail_open_active()`**.

<p class="txn-api-index" markdown="1">

**Methods:** [`configured()`](#poolruntimeconfigured) · [`configured_count()`](#poolruntimeconfigured_count) · [`eligible_count()`](#poolruntimeeligible_count) · [`fail_open_active()`](#poolruntimefail_open_active) · [`min_latency_ewma_ms()`](#poolruntimemin_latency_ewma_ms) · [`max_outstanding()`](#poolruntimemax_outstanding)

</p>

<div class="txn-api-entry" markdown="1">

### `PoolRuntime.configured()` {#poolruntimeconfigured}

<div class="txn-api-brief" markdown="1">

Returns `bool` — `true` when the pool exists in the active snapshot.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- `false` for unknown pool names — other **`PoolRuntime`** fields on that view are empty or zero.
- `true` when the pool is defined in config, even when **`health.enabled`** is false (counts then reflect health-off semantics).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `PoolRuntime.configured_count()` {#poolruntimeconfigured_count}

<div class="txn-api-brief" markdown="1">

Returns `i64` — number of backends defined on the pool in config.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Counts all **`pools[].backends`** entries — not only currently eligible backends.
- Compare with **`eligible_count()`** to detect partial outage or drain.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `PoolRuntime.eligible_count()` {#poolruntimeeligible_count}

<div class="txn-api-brief" markdown="1">

Returns `i64` — backends with **`applied == up`** (Route eligibility semantics).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Matches the eligible set [Route](/concepts/architecture-and-packet-path.md#route) uses when health gating is active.
- When pool health is disabled, equals **`configured_count()`** (all backends treated eligible).
- When **`fail_open_active()`** is true, Route may still send traffic to ineligible backends — scripts should check both when diagnosing behavior.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `PoolRuntime.fail_open_active()` {#poolruntimefail_open_active}

<div class="txn-api-brief" markdown="1">

Returns `bool` — Route is ignoring health gating for this pool (all-down or below **`min_eligible`** floor).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- `true` when eligible backends fall below **`pools[].health.min_eligible`** (or the pool has at most one backend and health gating would block all traffic).
- Route may forward to backends with **`applied == down`** while fail-open is active — **`eligible_count()`** alone does not predict the next hop.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `PoolRuntime.min_latency_ewma_ms()` {#poolruntimemin_latency_ewma_ms}

<div class="txn-api-brief" markdown="1">

Returns `()` (unit) when unset, else `float` — minimum [latency EWMA](/glossary/index.md#ewma) among pool backends with samples.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Aggregates probe [latency EWMA](/glossary/index.md#ewma) across backends in the pool that have at least one sample.
- Returns Rhai **unit** `()` when no backend in the pool has [EWMA](/glossary/index.md#ewma) data yet.
- Read-only hint for latency-aware branching — Route uses per-backend **`weight_factor()`** for weighted selection, not this pool-level minimum alone.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `PoolRuntime.max_outstanding()` {#poolruntimemax_outstanding}

<div class="txn-api-brief" markdown="1">

Returns `i64` — maximum in-flight forwards to any backend in the pool at hook entry.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Snapshot of concurrent upstream forwards per backend address at hook entry on this worker.
- Useful for overload or hot-spot detection before choosing a pool or retry target.
- Does not include queries still in earlier pipeline stages — only active forward attempts counted in the runtime view.

</div>

</div>

---

## Backend view (`BackendRuntime`) { #backend-view-backendruntime }

Returned by **`runtime.routing().backend(pool, id)`** or **`backend_for_attempt(...)`**. Methods below read **fields** for that named pool/backend pair.

**`id`** is the configured backend **`name`** when set and unique in the pool, otherwise **`ip:port`** (same rules as metrics labels and **`conduitctl health`**).

<p class="txn-api-index" markdown="1">

**Methods:** [`configured()`](#backendruntimeconfigured) · [`applied()`](#backendruntimeapplied) · [`observed()`](#backendruntimeobserved) · [`eligible()`](#backendruntimeeligible) · [`frozen()`](#backendruntimefrozen) · [`weight_factor()`](#backendruntimeweight_factor) · [`outstanding()`](#backendruntimeoutstanding) · [`latency_ewma_ms()`](#backendruntimelatency_ewma_ms) · [`last_transition_unix_ms()`](#backendruntimelast_transition_unix_ms)

</p>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.configured()` {#backendruntimeconfigured}

<div class="txn-api-brief" markdown="1">

Returns `bool` — `true` when the pool/backend pair exists in the active snapshot.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- `false` when the pool is unknown, the backend id does not resolve, or **`backend_for_attempt`** was called with empty strings.
- `true` for configured backends even when health is disabled ( **`applied`** is then `"up"` with **`observed`** `"unknown"`).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.applied()` {#backendruntimeapplied}

<div class="txn-api-brief" markdown="1">

Return `string` — `"up"`, `"down"`, or `"unknown"`.

**`applied`** is what Route uses.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- **`applied`** is the health state Route uses for eligibility and weighted selection.
- Operator drain/freeze via **`conduitctl health`** sets **`applied`** to **`down`** even when probes still report **`observed == up`**.
- Compare with **`observed()`** when diagnosing probe vs operator override.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.observed()` {#backendruntimeobserved}

<div class="txn-api-brief" markdown="1">

Return `string` — `"up"`, `"down"`, or `"unknown"`.

Probe/passive truth (may differ from **`applied`** when frozen/drained).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Reflects active/passive probe outcomes before operator overrides.
- May differ from **`applied()`** when the backend is frozen or drained — Route follows **`applied`**, not **`observed`**.
- **`unknown`** when no probe sample exists yet for this backend.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.eligible()` {#backendruntimeeligible}

<div class="txn-api-brief" markdown="1">

Returns `bool` — `true` when **`applied == up`**.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Equivalent to **`applied() == "up"`** — shorthand for Route eligibility checks.
- `false` does not guarantee Route will skip this backend when **`fail_open_active()`** is true on the pool.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.frozen()` {#backendruntimefrozen}

<div class="txn-api-brief" markdown="1">

Returns `bool` — operator freeze/drain scope active for this backend.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- `true` when an operator **`conduitctl health`** freeze/drain applies to this pool/backend.
- Often pairs with **`observed() == "up"`** and **`applied() == "down"`** — probes still succeed but Route treats the backend as down.
- Scripts should not call the control plane per query; read **`frozen()`** for policy branching only.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.outstanding()` {#backendruntimeoutstanding}

<div class="txn-api-brief" markdown="1">

Returns `i64` — in-flight upstream forwards to this backend at hook entry.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Per-backend concurrent forward count on this worker at hook entry.
- `0` when idle — use with **`max_outstanding()`** on the pool for pool-wide load signals.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.weight_factor()` {#backendruntimeweight_factor}

<div class="txn-api-brief" markdown="1">

Returns `float` — damped latency weight factor [Route](/concepts/architecture-and-packet-path.md#route) applies (1.0 = no reduction). Derived from [EWMA](/glossary/index.md#ewma).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Range **0.0–1.0** after [EWMA](/glossary/index.md#ewma)-based latency damping — **`1.0`** means full configured weight.
- Lower values reduce selection probability for slower backends; updated by the probe scheduler, not by Rhai.
- Read-only — scripts cannot change weights through **`runtime`**.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.latency_ewma_ms()` {#backendruntimelatency_ewma_ms}

<div class="txn-api-brief" markdown="1">

Returns `()` when no probe sample yet, else `float` — [latency EWMA](/glossary/index.md#ewma) in milliseconds.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- [EWMA](/glossary/index.md#ewma) of successful health-probe round-trip time for this backend (recent samples weigh more than older ones).
- Returns Rhai **unit** `()` until the first probe sample exists.
- Drives **`weight_factor()`** over time — useful for logging or canary metrics, not for per-query weight overrides.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `BackendRuntime.last_transition_unix_ms()` {#backendruntimelast_transition_unix_ms}

<div class="txn-api-brief" markdown="1">

Returns `()` when unknown, else `i64` — Unix ms of last observed/applied transition.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Milliseconds since Unix epoch when **`observed`** or **`applied`** last changed for this backend.
- Returns Rhai **unit** `()` when no transition has been recorded yet.
- Use for staleness checks (for example avoid retry storms immediately after a backend flaps).

</div>

</div>

---

## Operator health vs script reads

| Mechanism | Path | Use |
|-----------|------|-----|
| **`conduitctl health`** / gRPC | Control plane | Drain, freeze, resume — changes **`applied`** and scope |
| **`runtime.routing()`** | Hot path (Rhai) | Read **`applied`**, **`eligible`**, pool-wide counts for **policy** |
| Probe logs | `backend health transition` INFO | Observe probe-driven changes in process logs |

Scripts should **not** call the control plane per query. Use **`runtime.routing()`** for branching; use **`conduitctl`** for operational drain/freeze.

## Related

- [Host API overview](/rhai/host-api.md)
- [Transaction API (`txn`)](/rhai/txn-api.md) — policy writes
- [Pools and backends](/policy-routing/pools-and-backends.md) — config model
- [gRPC and conduitctl — health](/control-plane/grpc-and-conduitctl.md) — operator controls
