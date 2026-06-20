---
toc_depth: 3
---

# Transaction API

**Rhai for rules** ([Rule Rhai](/rhai/rule-rhai.md)) scripts receive a sandboxed **`txn`** object. Methods on **`txn`** set policy on the current [transaction](/glossary/index.md#transaction) — pools, [tags](/glossary/index.md#tags), drop/retry intent, egress overrides, and observability side effects. They do **not** edit DNS wire bytes (see [Rhai for processor chains](/rhai/processor-chain-rhai.md) for wire editing).

For which hook each API allows, see [Hooks and phases](/rhai/hooks-and-phases.md#phase-guards) (summary table) and [Request vs response](/rhai/hooks-and-phases.md#request-vs-response-script-perspective) (when each hook runs). This page documents each method in depth: arguments, behavior, YAML equivalents, and examples.

## How to read this page

Each entry uses the same sections:

| Section | Meaning |
|---------|---------|
| **Hooks** | Whether the method is available on the [request hook](/rhai/hooks-and-phases.md#request-hook), [response hook](/rhai/hooks-and-phases.md#response-hook), or both |
| **Arguments / return** | Rhai types and what the call returns |
| **Behavior** | What changes on the transaction and how it interacts with the pipeline |
| **YAML equivalent** | Built-in action on a rule’s `actions:` list, if one exists |
| **Example** | Typical script usage |

**Hook names** — Rhai for rules runs at two pipeline points. The [request hook](/rhai/hooks-and-phases.md#request-hook) runs once per transaction before upstream [Route](/concepts/architecture-and-packet-path.md#route); the [response hook](/rhai/hooks-and-phases.md#response-hook) runs after each forward attempt. For script-author detail (retry behavior, phase guards, pairing request/response scripts), see [Hooks and phases](/rhai/hooks-and-phases.md#request-vs-response-script-perspective). For YAML `hook: request` / `hook: response` wiring and outcomes after each hook, see [Rules and actions — Request and response hooks](/policy-routing/rules-and-actions.md#request-and-response-hooks).

Methods are grouped by purpose inside bordered cards. Groups appear in **alphabetical** order; each group lists its methods under the group heading. Entries marked *in progress* do not have full cards yet.

---

## Egress

At [Forward](/concepts/architecture-and-packet-path.md#forward), Conduit resolves the local bind address per address family:

1. **Retry forward** (`attempt_count > 1` at Forward) — if **`retry_source_override_v4`** / **`retry_source_override_v6`** is set, use it **once** (then clear the stash).
2. Otherwise — standing **`source_override_v4`** / **`source_override_v6`** from **`set_source_*`** on the request hook.
3. Otherwise — round-robin among configured pool/global sources.

Pool choice and egress source are **decoupled** — per-pool `sources_v4` / `sources_v6` is the main pool→egress mapping; **`set_retry_source_*`** is for per-query, outcome-driven overrides on retry forwards only. See [Source selection lifecycle](/policy-routing/retries-and-transactions.md#source-selection-lifecycle).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.set_source_v4(addr)`](#txnset_source_v4addr) · [`txn.set_source_v6(addr)`](#txnset_source_v6addr) · [`txn.set_retry_source_v4(addr)`](#txnset_retry_source_v4addr) · [`txn.set_retry_source_v6(addr)`](#txnset_retry_source_v6addr) · [`txn.clear_retry_source_v4()`](#txnclear_retry_source_v4) · [`txn.clear_retry_source_v6()`](#txnclear_retry_source_v6)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.set_source_v4(addr)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) only

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv4 address (for example `"10.0.0.5"`) |
| *return* | — | No return value on success |

Returns a **script error** (phase guard or parse failure) when called on the wrong hook or with an invalid address — see **Behavior**.

#### Behavior

- Sets **`source_override_v4`** on the [transaction](/glossary/index.md#transaction) — the local IPv4 address Conduit uses when binding for upstream [Forward](/concepts/architecture-and-packet-path.md#forward) to an **IPv4** [backend](/glossary/index.md#backend).
- Runs **before** Route on the request hook; the override applies to **every** forward attempt on this transaction unless a one-shot **`retry_source_override_*`** wins on a retry forward (see resolution order above).
- At Forward, Conduit uses the override only when it is in the **allowed set** for the selected [pool](/glossary/index.md#pool): `forward.sources_v4` ∪ that pool’s `sources_v4` (when the pool list is non-empty, pool sources apply for round-robin; the allowed union is still checked for overrides). If the address is **not** allowed, Conduit **does not fail the query** — it falls back to ordinary round-robin among configured sources. Same rule as built-in **`set_source_v4`**. See [Dual-stack forwarding — Choosing an egress source](/guides/dual-stack-forwarding.md#choosing-an-egress-source).
- **YAML** `set_source_v4` values are checked against the global union at validate/reload. **Rhai** accepts any parseable IPv4 string at runtime — allowed-set enforcement happens at Forward, not at script compile time. Use fixed literals you have pre-validated in config, or accept fail-open fallback when an address is wrong for the pool.
- **Response hook:** calling this method is a **phase error** — Conduit logs **`rhai script error`**, skips further script effects for that hook invocation, and continues the pipeline ([Sandbox limits](/rhai/sandbox-limits.md) — fail-open). Egress is already chosen before Forward on that attempt.
- Pair with **`txn.set_pool`** when both matter: built-in actions on the same rule should list **`set_pool` first** so Forward checks the override against the pool you selected; a **`rhai`** step at position *N* sees prior built-in effects. See [Action order on one rule](/policy-routing/rules-and-actions.md#action-order-on-one-rule).
- Independent of **`txn.set_source_v6`** — Forward picks v4 or v6 override to match the backend address family.

#### YAML equivalent

```yaml
- type: set_source_v4
  value: "10.0.0.5"
```

#### Example

Pin egress from a lookup table on the request hook:

```rhai
let egress = table_lookup("egress_map", question_qname(txn));
if egress != "" {
    txn.set_source_v4(egress);
}
```

Fixed literal (repository fixture pattern — address must be in `forward.sources_v4` or pool `sources_v4` for the override to take effect):

```rhai
txn.set_source_v4("127.0.0.1");
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_source_v6(addr)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) only

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv6 address (for example `"2001:db8::1"` or `"::1"`) |
| *return* | — | No return value on success |

Returns a **script error** (phase guard or parse failure) when called on the wrong hook or with an invalid address — see **Behavior**.

#### Behavior

- Sets **`source_override_v6`** on the [transaction](/glossary/index.md#transaction) — the local IPv6 address Conduit uses when binding for upstream [Forward](/concepts/architecture-and-packet-path.md#forward) to an **IPv6** [backend](/glossary/index.md#backend).
- Runs **before** Route on the request hook; the override applies to **every** forward attempt on this transaction unless a one-shot **`retry_source_override_*`** wins on a retry forward (see resolution order above).
- At Forward, Conduit uses the override only when it is in the **allowed set** for the selected [pool](/glossary/index.md#pool): `forward.sources_v6` ∪ that pool’s `sources_v6`. If the address is **not** allowed, Conduit falls back to round-robin among configured sources — same as built-in **`set_source_v6`**. See [Dual-stack forwarding — Choosing an egress source](/guides/dual-stack-forwarding.md#choosing-an-egress-source).
- **YAML** `set_source_v6` values are checked at validate/reload. **Rhai** enforces parse + request-hook phase only; allowed-set enforcement is at Forward.
- **Response hook:** phase error — same fail-open behavior as **`txn.set_source_v4`**.
- Pair with **`txn.set_pool`** when both matter — list **`set_pool` before** source actions on the same rule when using built-ins; scripts see built-in effects that ran earlier in the action list. See [Action order on one rule](/policy-routing/rules-and-actions.md#action-order-on-one-rule).
- Independent of **`txn.set_source_v4`** — only the override matching the backend family is used.

#### YAML equivalent

```yaml
- type: set_source_v6
  value: "::1"
```

#### Example

Request hook — pin IPv6 egress for queries routed to an IPv6 backend pool:

```rhai
if question_qname(txn).ends_with(".v6.example.") {
    txn.set_pool("v6-upstream");
    txn.set_source_v6("::1");
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_retry_source_v4(addr)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv4 address (for example `"10.0.0.5"`) |
| *return* | — | No return value on success |

Returns a **script error** on invalid address parse failure.

#### Behavior

- Sets **`retry_source_override_v4`** — a **one-shot** local IPv4 bind for the **next retry forward only** (`attempt_count > 1` at [Forward](/concepts/architecture-and-packet-path.md#forward)).
- Does **not** trigger [retry](/glossary/index.md#retry). Pair with **`txn.request_retry()`** / **`txn.request_retry_now()`** on the response hook (or built-in **`retry`** / **`retry_now`**) when you want failover to use this egress.
- On the **first** forward (`attempt_count == 1` at Forward), the stash is **ignored** — standing **`source_override_v4`** or pool/global round-robin applies instead.
- On a **retry** forward, this override **wins over** standing **`source_override_v4`** for that attempt only (then the stash is cleared). Same allowed-set check at Forward as **`set_source_v4`**; disallowed addresses fail open to round-robin.
- **YAML** `set_retry_source_v4` values are checked against the global `sources_v4` union at validate/reload (both hooks). **Rhai** enforces parse only at runtime; allowed-set enforcement is at Forward.
- Typical pattern: request rule sets standing egress with **`txn.set_source_v4`**; response rule sets **`txn.set_retry_source_v4`** when upstream outcome warrants a different bind on retry.

#### YAML equivalent

```yaml
- type: set_retry_source_v4
  value: "10.0.0.5"
```

#### Example

Response hook — alternate egress on SERVFAIL retry:

```rhai
if txn.response_rcode() == "SERVFAIL" {
    txn.set_retry_source_v4("10.0.0.5");
    txn.request_retry();
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_retry_source_v6(addr)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv6 address (for example `"2001:db8::1"` or `"::1"`) |
| *return* | — | No return value on success |

Returns a **script error** on invalid address parse failure.

#### Behavior

- Sets **`retry_source_override_v6`** — one-shot local IPv6 bind for the **next retry forward only** (`attempt_count > 1` at Forward).
- Does **not** trigger retry. Pair with **`txn.request_retry()`** / **`txn.request_retry_now()`** when failover should use this egress.
- First forward ignores the stash; standing **`source_override_v6`** or pool/global round-robin applies.
- On retry forward, wins over standing **`source_override_v6`** for one attempt, then clears. Same allowed-set and fail-open rules as **`set_source_v6`**.
- Independent of **`txn.set_retry_source_v4`** — only the override matching the backend family is used.

#### YAML equivalent

```yaml
- type: set_retry_source_v6
  value: "::1"
```

#### Example

Request hook — pre-stash alternate v6 egress before first forward (used only if a later retry occurs):

```rhai
txn.set_retry_source_v6("::1");
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry_source_v4()`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. No return value.

#### Behavior

- Clears **`retry_source_override_v4`** on the [transaction](/glossary/index.md#transaction).
- Does **not** clear standing **`source_override_v4`** from **`set_source_v4`**.
- Use when a stashed retry egress should not apply (for example after deciding same-pool **`retry`** without changing bind IP).

#### YAML equivalent

```yaml
- type: clear_retry_source_v4
  value: ""
```

#### Example

```rhai
txn.clear_retry_source_v4();
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry_source_v6()`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. No return value.

#### Behavior

- Clears **`retry_source_override_v6`** on the transaction.
- Does **not** clear standing **`source_override_v6`** from **`set_source_v6`**.

#### YAML equivalent

```yaml
- type: clear_retry_source_v6
  value: ""
```

#### Example

```rhai
txn.clear_retry_source_v6();
```

</div>

---

## Lookups

Host-owned lookup tables from **`data_sources:`** in config. **`table_lookup`** is a **top-level** Rhai function — not a method on **`txn`**. Scripts can only read tables you declare in config; arbitrary file access is not available. Config, CSV format, reload, and validation: [Data sources and lookups](/rhai/data-sources-and-lookups.md).

<p class="txn-api-index" markdown="1">

**Functions:** [`table_lookup(table, key)`](#table_lookuptable-key)

</p>

<div class="txn-api-entry" markdown="1">

### `table_lookup(table, key)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `table` | string | **`name`** of a `data_sources:` entry (for example `"blocklist"`, `"geo"`) |
| `key` | string | Lookup key — typically `question_qname(txn)` for qname-keyed CSVs |
| *return* | string | Value from the table, or **`""`** (empty string) on miss |

There is no YAML equivalent — declare the table under **`data_sources:`** and call **`table_lookup`** from a **`rhai`** action.

#### Behavior

- Reads from an in-memory map built when the [runtime snapshot](/glossary/index.md#runtime-snapshot) is compiled — not from disk on each query.
- **Grant model:** only tables listed under top-level **`data_sources:`** are visible.
- **Miss vs unknown table:** a missing **key** returns **`""`** silently. An unknown **table** name also returns **`""`** so scripts keep running, but Conduit logs (milestone counts 1, 10, 100, … plus at most once per 60s while hits continue) and increments [`conduit_script_errors_total`](/observability/built-in-metrics.md#conduit_script_errors_total) with `reason="lookup_unknown_table"`. Literal table names in source are rejected at compile time — see [Data sources and lookups](/rhai/data-sources-and-lookups.md#table_lookup-behavior).
- **Miss semantics:** missing key or empty value cell return **`""`**. Scripts should treat empty string as “no mapping” (see examples).
- **Reload:** when config reload builds a new snapshot, CSV files are re-read and tables replace the prior generation. In-flight transactions keep the snapshot they started with; new queries see the updated tables. See [Data sources and lookups — Reload](/rhai/data-sources-and-lookups.md#reload-and-snapshot).
- **Case and matching:** lookup is **exact** string match on the key column value — include the trailing dot on FQDN-style qnames if your CSV keys use it (`bad.example.` not `bad.example`).
- Counts toward [sandbox limits](/rhai/sandbox-limits.md) like any other host call (`max_operations`, `hook_timeout_ms`).
- Typical uses: block/allow lists, qname→pool or qname→egress maps, region tags for metrics or event export. Pair with **`question_qname(txn)`** on the request hook before [Route](/concepts/architecture-and-packet-path.md#route); use response hook when branching on upstream outcome **and** a table (less common).

#### Config (not YAML action)

```yaml
data_sources:
  - name: blocklist
    type: csv
    path: data/blocklist.csv
    key_column: qname
    value_column: action
```

Paths are relative to the config file directory unless absolute. Full field reference: [Data sources and lookups](/rhai/data-sources-and-lookups.md).

#### Example

Block when CSV maps qname to `block` (repository fixture):

```rhai
if table_lookup("blocklist", question_qname(txn)) == "block" {
    txn.drop_query_now();
}
```

Tag from geo table when region is present:

```rhai
let region = table_lookup("geo", question_qname(txn));
if region != "" {
    txn.set_tag("region", region);
}
```

Pin egress from a qname-keyed map on the request hook:

```rhai
let egress = table_lookup("egress_map", question_qname(txn));
if egress != "" {
    txn.set_source_v4(egress);
}
```

Runnable configs in the repository: `tests/fixtures/config/with-rhai-blocklist.yaml`, `with-rhai-lookup-demo.yaml` — see `tests/fixtures/rhai/README.md` for runnable Rhai fixture examples.

</div>

---

## Metrics and timing

Wall-clock time and forward [attempt](/glossary/index.md#retry) count on the current [transaction](/glossary/index.md#transaction); custom policy counters (`conduit_user_*`). Registration, export tiers, and Prometheus naming: [User metrics](/rhai/user-metrics.md).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.elapsed_ms()`](#txnelapsed_ms) · [`txn.get_attempt_count()`](#txnget_attempt_count) · [`txn.last_forward_ms()`](#txnlast_forward_ms) · [`txn.metric_inc(name, delta)`](#txnmetric_incname-delta) · [`txn.metric_inc_labels(name, delta, labels)`](#txnmetric_inc_labelsname-delta-labels)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.elapsed_ms()`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns **`i64`** — milliseconds elapsed since the transaction started.

#### Behavior

- Measures **wall-clock time** from transaction creation (when Conduit accepted the client query) through the current hook invocation.
- Includes time spent in earlier pipeline phases on this transaction: [request rules](/concepts/architecture-and-packet-path.md#request-rules), [Route](/concepts/architecture-and-packet-path.md#route), [Forward](/concepts/architecture-and-packet-path.md#forward), [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response), and any prior [response rules](/concepts/architecture-and-packet-path.md#response-rules) passes on [retries](/glossary/index.md#retry).
- **Not** upstream RTT alone — use [`txn.last_forward_ms()`](#txnlast_forward_ms) for the most recent forward attempt’s upstream wait.
- On the [request hook](/rhai/hooks-and-phases.md#request-hook), elapsed time is usually small (rules only, no upstream wait yet).
- On the [response hook](/rhai/hooks-and-phases.md#response-hook), elapsed time includes the wait for the current forward attempt’s answer or timeout.
- The same clock backs transaction duration limits in [Retries and transactions](/policy-routing/retries-and-transactions.md) (`max_txn_duration_ms` when configured).
- Read-only — does not change the transaction.

#### YAML equivalent

None.

#### Example

Response script — increment a user metric when a tagged query had a slow **upstream** forward (repository fixture `slow-login-alert.rhai` / `with-rhai-slow-login.yaml`):

```rhai
if txn.has_tag("suspicious") && txn.last_forward_ms() > 500 {
    txn.metric_inc("slow_login", 1);
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.last_forward_ms()`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns **`i64`** — milliseconds for the **most recent** upstream forward attempt on this transaction.

#### Behavior

- Measures upstream **send → answer or timeout** time for the latest [Forward](/concepts/architecture-and-packet-path.md#forward) attempt — the same interval recorded in [`conduit_forward_duration_seconds`](/observability/built-in-metrics.md#conduit_forward_duration_seconds) when metrics are enabled.
- **Not** end-to-end transaction time — use [`txn.elapsed_ms()`](#txnelapsed_ms) for wall-clock time since the client query arrived (includes request rules, prior retries, and response-rule passes).
- **Request hook:** always **`0`** — no forward attempt has completed yet.
- **Response hook:** set after the current attempt’s forward completes (success, timeout, or forward error that still runs response rules). On [retry](/glossary/index.md#retry), each new attempt **overwrites** the value with that attempt’s RTT.
- Includes TCP fallback time when UDP returns **TC** and Conduit retries over TCP on the same attempt.
- Timeout attempts record approximately **`forward.timeout_ms`** (see [Forward](/concepts/architecture-and-packet-path.md#forward)).
- Hard forward failures that skip [Response rules](/concepts/architecture-and-packet-path.md#response-rules) (for example immediate **SERVFAIL** to [Send](/concepts/architecture-and-packet-path.md#send)) still set the value on the transaction, but response-hook scripts do not run for that pass.
- Read-only — does not change the transaction.
- Available regardless of `metrics.enabled` — this is per-transaction state for policy, not Prometheus export.

#### YAML equivalent

None.

#### Example

Retry only when the latest upstream attempt was slow:

```rhai
if txn.last_forward_ms() > 800 && txn.get_attempt_count() == 1 {
    txn.request_retry();
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.get_attempt_count()`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns **`i64`** — the transaction’s forward **attempt count** at hook entry.

#### Behavior

- Reflects how many times [Route](/concepts/architecture-and-packet-path.md#route) has selected a pool and recorded a forward attempt on this transaction. Increment happens at Route, **before** [Forward](/concepts/architecture-and-packet-path.md#forward) for that attempt.
- **Request hook:** always **`0`** — no Route has run yet.
- **First response hook** (after one upstream round trip): **`1`**.
- **Second response hook** (after one retry): **`2`**, and so on.
- Use on the response hook to branch on first vs subsequent upstream outcomes (for example only retry once, or different metrics per attempt). Pair with [Retries and transactions](/policy-routing/retries-and-transactions.md) and [`txn.request_retry()`](/rhai/transaction-api.md#outcomes).
- Read-only — does not change the transaction.
- The value matches **`attempt_count`** on [event export](/observability/event-export.md) extra fields when that field is enabled.

#### YAML equivalent

None. Declarative rules do not expose attempt count as a selector today.

#### Example

Response script — act only on the first upstream failure:

```rhai
if txn.get_attempt_count() == 1 && txn.response_rcode() == "SERVFAIL" {
    txn.request_retry();
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.metric_inc(name, delta)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `name` | string | Metric name (ASCII letters, digits, `_`); exported as `conduit_user_<name>` |
| `delta` | integer | Non-negative increment; values **&lt; 0** are treated as **0** |
| *return* | — | No return value on success |

#### Behavior

- Increments a **user-defined counter** discovered at snapshot compile from `metric_inc("name", …)` in Rhai source. Full registration rules: [User metrics — Declaring metrics](/rhai/user-metrics.md#declaring-metrics-in-scripts).
- Equivalent to **`txn.metric_inc_labels(name, delta, #{})`** — use when the metric has **no** label keys.
- Increments are **buffered** for the current hook run and flushed after a **successful** script completion when `metrics.enabled` is true and the metric’s [export tier](/rhai/user-metrics.md#export-tier) matches `metrics.profile`. Filtered metrics are dropped silently at export — the call still succeeds.
- Scripts **cannot read** counter values back; use [tags](/rhai/transaction-api.md#tags) or txn state for per-query policy.
- **Errors** (failed script evaluation — see [Script errors on a hook](/rhai/hooks-and-phases.md#script-errors-on-a-hook)):
  - Unknown `name` (not registered at compile)
  - Disallowed or unexpected label keys (unlabeled form only passes when the metric has no registered label keys)
- Counts toward [sandbox limits](/rhai/sandbox-limits.md) like any host call.

#### YAML equivalent

None.

#### Example

```rhai
txn.metric_inc("slow_login", 1);
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.metric_inc_labels(name, delta, labels)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `name` | string | Metric name; exported as `conduit_user_<name>` |
| `delta` | integer | Non-negative increment; values **&lt; 0** are treated as **0** |
| `labels` | map | Rhai map literal `#{ key: value, … }` — label keys must match compile-time registration |
| *return* | — | No return value on success |

#### Behavior

- Same flush and export behavior as **`txn.metric_inc`**, but attaches **labels** to the increment.
- Label **keys** are discovered at compile from the `#{ … }` map on `metric_inc` / `metric_inc_labels` calls across all scripts. Keys must be **consistent** for a given metric name; conflicting sets fail snapshot build.
- **Disallowed label keys** (high cardinality): `qname`, `client`, `client_ip`, `client_addr`, `backend`, `txn_id`, `dns_id`, `address`, `ip`, `host`, `query`, `zone`, `fqdn` — rejected at compile and runtime.
- **Runtime errors:** unknown metric name; label key not registered for that metric; disallowed label key.
- Label values are converted with `to_string()` for export.
- Opt in metrics on **`minimal`** deployments with `metrics.user_metrics` — see [User metrics — Export tier](/rhai/user-metrics.md#export-tier).

#### YAML equivalent

None.

#### Example

Geo-tagged block counter (repository fixture `block-hits.rhai` / `with-rhai-block-hits.yaml`):

```rhai
let cat = table_lookup("geo", question_qname(txn));
if cat == "eu" {
    txn.metric_inc_labels("block_hits", 1, #{ category: "eu" });
} else if cat == "us" {
    txn.metric_inc_labels("block_hits", 1, #{ category: "us" });
}
```

</div>

---

## Outcomes

<p class="txn-api-index" markdown="1">

**Methods:** `txn.clear_drop()` · `txn.clear_retry()` · `txn.drop_query()` · `txn.drop_query_now()` · `txn.request_retry()` · `txn.request_retry_now()` · `txn.set_rcode(name)` *in progress*

</p>

<p class="txn-api-stub" markdown="1">

Drop, retry, and response metadata on the [transaction](/glossary/index.md#transaction). Soft vs hard effects and how they combine on one rule: [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule). Retry caps and pool behavior: [Retries and transactions](/policy-routing/retries-and-transactions.md).

Method cards for this group are not written yet. Hook availability: [Hooks and phases — phase guards](/rhai/hooks-and-phases.md#phase-guards).

</p>

---

## Query and response

<p class="txn-api-index" markdown="1">

**Methods:** `question_qname(txn)` · `txn.question()` · `txn.response()` · `txn.response_rcode()` *in progress*

</p>

<p class="txn-api-stub" markdown="1">

Read-only access to the client question and (on the [response hook](/rhai/hooks-and-phases.md#response-hook)) upstream outcome. `txn.response()` is not available on the [request hook](/rhai/hooks-and-phases.md#request-hook); `txn.response_rcode()` returns an empty string on the request hook.

Method cards for this group are not written yet. Hook availability: [Hooks and phases — phase guards](/rhai/hooks-and-phases.md#phase-guards).

</p>

---

## Routing { #routing }

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.clear_retry_pool()`](#txnclear_retry_pool) · [`txn.set_pool(name)`](#txnset_poolname) · [`txn.set_retry_pool(name)`](#txnset_retry_poolname)

</p>

<p class="txn-api-stub" markdown="1">

**`set_pool`** and **`set_retry_pool`** interact over multiple [Route](/concepts/architecture-and-packet-path.md#route) attempts — first forward vs retry, one-shot **`retry_pool`**, and how **`selected_pool`** updates after each Route. See [Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle).

</p>

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry_pool()`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

#### Behavior

- Clears the **`retry_pool`** field on the [transaction](/glossary/index.md#transaction) — the pool name stashed by **`txn.set_retry_pool`** or built-in **`set_retry_pool`**.
- Does **not** clear soft-retry intent from **`txn.request_retry()`** or built-in **`retry`**. Use **`txn.clear_retry()`** on the [response hook](/rhai/hooks-and-phases.md#response-hook) for that.
- Does **not** change **`selected_pool`** (the pool from **`txn.set_pool`**, request rules, or the default pool). A retry that still occurs uses **`selected_pool`** when **`retry_pool`** is absent at [Route](/concepts/architecture-and-packet-path.md#route). See [Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle).
- Within one rule, built-in actions run in list order; a **`rhai`** step sees prior effects and can clear a stash set earlier on the same rule.
- Typical use: undo an earlier **`set_retry_pool`** when this hook should retry in the **current** pool instead — for example request policy stashed a backup pool, but response policy matches **SERVFAIL** and you want same-pool failover only. See [Retry actions](/policy-routing/rules-and-actions.md#retry-actions).

#### YAML equivalent

```yaml
- type: clear_retry_pool
```

#### Example

Response script — retry in the current pool even though request policy stashed a backup pool:

```rhai
if txn.response_rcode() == "SERVFAIL" {
    txn.clear_retry_pool();
    txn.request_retry();
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_pool(name)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `name` | string | [Pool](/glossary/index.md#pool) name from config |
| *return* | — | No return value |

#### Behavior

- Writes **`selected_pool`** on the [transaction](/glossary/index.md#transaction) — the primary pool for [Route](/concepts/architecture-and-packet-path.md#route).
- On the [request hook](/rhai/hooks-and-phases.md#request-hook), the **first** Route uses this value before [Forward](/concepts/architecture-and-packet-path.md#forward).
- On later Routes, Conduit uses **`selected_pool`** when **`retry_pool`** is unset (after a one-shot stash is consumed, **`selected_pool`** reflects the pool from the **last** Route — not necessarily the original request value). See [Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle).
- On the [response hook](/rhai/hooks-and-phases.md#response-hook), a pool change affects Route only if policy triggers a [retry](/glossary/index.md#retry) — otherwise the query is past routing for this attempt.
- If multiple rules or actions set a pool, the **last** writer on the winning rule wins. Within one rule, built-in actions run in list order; a **`rhai`** step at position *N* sees effects from actions *1…N−1* and can override them (for example `txn.set_pool("vip")` after YAML `set_pool: default`).
- This is **`set_pool`**, not **`set_retry_pool`**. Use **`txn.set_retry_pool`** when you intend the pool for a **retry** Route while leaving the first Route on the current pool.

#### YAML equivalent

```yaml
- type: set_pool
  value: vip
```

See [Pools and backends](/policy-routing/pools-and-backends.md) for pool definitions, default pool, and backend selection.

#### Example

```rhai
let qname = question_qname(txn);
if qname.ends_with(".vip.example.") {
    txn.set_pool("vip");
} else if qname.ends_with(".slow.example.") {
    txn.set_pool("bulk");
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_retry_pool(name)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `name` | string | [Pool](/glossary/index.md#pool) name from config |
| *return* | — | No return value |

#### Behavior

- Stashes a pool name in **`retry_pool`** on the [transaction](/glossary/index.md#transaction). It does **not** trigger a [retry](/glossary/index.md#retry) by itself — pair with **`txn.request_retry()`** or **`txn.request_retry_now()`** on the [response hook](/rhai/hooks-and-phases.md#response-hook), or with built-in **`retry`** / **`retry_now`** on a response rule.
- On the **first** [Route](/concepts/architecture-and-packet-path.md#route) (`attempt_count == 0`), Conduit uses **`selected_pool`** from **`txn.set_pool`**, request rules, or the default pool — **`retry_pool` is ignored**. The stash remains for a later retry.
- On a **retry** Route (`attempt_count > 0`), Conduit consumes **`retry_pool`** once and routes in that pool; if **`retry_pool`** was cleared or never set, Route uses **`selected_pool`**. After that Route, **`selected_pool`** updates to the pool used — you usually do **not** need to stash again to stay on a failover pool. See [Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle).
- Available on both hooks — common on the [request hook](/rhai/hooks-and-phases.md#request-hook) to pre-stage a backup pool before [Forward](/concepts/architecture-and-packet-path.md#forward), and on the [response hook](/rhai/hooks-and-phases.md#response-hook) when upstream outcome decides failover.
- Within one rule, later **`set_retry_pool`** or **`clear_retry_pool`** (built-in or Rhai) overrides an earlier stash on the same hook pass. **`txn.request_retry()`** has no effect on the request hook.
- This is **`set_retry_pool`**, not **`set_pool`**. Use **`txn.set_pool`** to change the pool for the **first** forward.

#### YAML equivalent

```yaml
- type: set_retry_pool
  value: secondary
```

Pair with **`retry`** or **`retry_now`** on a response rule to fail over — see [Retry actions](/policy-routing/rules-and-actions.md#retry-actions).

#### Example

Response script — fail over to a backup pool on **SERVFAIL** (same pattern as `tests/fixtures/rhai/servfail-retry.rhai` in the repository):

```rhai
if txn.response_rcode() == "SERVFAIL" {
    txn.set_retry_pool("secondary");
    txn.request_retry();
}
```

Request hook — route to **primary** now, stash **secondary** if a later retry occurs (built-in actions can do the same; see `tests/fixtures/config/with-rhai-servfail-retry.yaml`):

```rhai
txn.set_pool("primary");
txn.set_retry_pool("secondary");
```

</div>

---

## Sampling

<p class="txn-api-index" markdown="1">

**Methods:** `txn.sample_percent(percent)` · `txn.sample_percent(percent, key)` *in progress*

</p>

<p class="txn-api-stub" markdown="1">

Deterministic per-[transaction](/glossary/index.md#transaction) sampling for audit tags and script-side gates. Keyed sampling matches YAML `sample_percent` selectors: [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence).

Method cards for this group are not written yet. Hook availability: [Hooks and phases — phase guards](/rhai/hooks-and-phases.md#phase-guards).

</p>

---

## Tags

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.clear_tag(key)`](#txnclear_tagkey) · [`txn.has_tag(key)`](#txnhas_tagkey) · [`txn.set_tag(key, value)`](#txnset_tagkey-value)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.clear_tag(key)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `key` | string | Tag name to remove |
| *return* | — | No return value |

#### Behavior

- Removes a [tag](/glossary/index.md#tags) key from the transaction — both boolean flags and string values for that key.
- Later calls with the same key in one script run follow **last write wins** semantics with `txn.set_tag` (for example `set_tag` then `clear_tag` leaves the key absent).
- Does **not** treat `txn.set_tag(key, false)` as a clear — use **`txn.clear_tag(key)`** explicitly when you mean removal.
- **`txn.has_tag(key)`** checks script effects first, then tags present at hook entry.

#### YAML equivalent

```yaml
- type: clear_tag
  value: suspicious
```

#### Example

```rhai
if txn.has_tag("temporary") {
    txn.clear_tag("temporary");
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.has_tag(key)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `key` | string | Tag name |
| *return* | `bool` | `true` when the tag is present under the rules below |

#### Behavior

- Read-only — does not change the [transaction](/glossary/index.md#transaction).
- Returns `true` when:
  - A **`txn.set_tag`** or **`txn.clear_tag`** earlier **in this script run** left the key present (last write wins within the script), or
  - The key was already on the transaction at **hook entry** (from built-in **`set_tag`** / **`clear_tag`** on this or an earlier matching rule, or from a prior hook on the same transaction).
- For **boolean** tags: `true` only when the bool flag is `true`. `txn.set_tag(key, false)` yields `false` for that key until a later `set_tag` or `clear_tag` in the same script.
- For **string** tags: `true` when any string value is stored for the key (including values set by YAML `set_tag: key=value`).
- Tags from the [request hook](/rhai/hooks-and-phases.md#request-hook) are still visible on the [response hook](/rhai/hooks-and-phases.md#response-hook) (the request hook does not re-run on [retry](/glossary/index.md#retry)).
- Declarative rules test tags with the **`tag`** [selector](/glossary/index.md#selector), not `has_tag`. See [Rules and actions — Selectors](/policy-routing/rules-and-actions.md#selectors).

#### YAML equivalent

None — use a **`tag`** selector on a rule to match a tag set elsewhere:

```yaml
selectors:
  - type: tag
    value: suspicious
```

#### Example

Response script — act only when the [request hook](/rhai/hooks-and-phases.md#request-hook) tagged the query (see [Hooks and phases — pairing scripts](/rhai/hooks-and-phases.md#pairing-request-and-response-scripts)):

```rhai
if txn.has_tag("suspicious") {
    if txn.last_forward_ms() > 500 {
        txn.metric_inc("slow_login", 1);
    }
}
```

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_tag(key, value)`

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `key` | string | Tag name |
| `value` | bool or string | `true` / `false` for boolean tags; any other value is stored as a string |
| *return* | — | No return value |

#### Behavior

- Sets a [tag](/glossary/index.md#tags) on the transaction. Tags persist for the rest of the transaction — including when the [response hook](/rhai/hooks-and-phases.md#response-hook) runs and across [retries](/glossary/index.md#retry) (the [request hook](/rhai/hooks-and-phases.md#request-hook) does not re-run on retry).
- Boolean tags use `true` / `false`. String tags store arbitrary text. **`txn.has_tag(key)`** is true when the bool flag is `true` or a string value is set for that key — pick one style per key in practice.
- Later calls with the same key overwrite the value from this script run. Built-in **`set_tag`** actions on the same rule that ran **before** the script are visible to **`txn.has_tag`**; the script can add or override tags after that.
- Tags are visible to downstream rules only if those rules’ selectors match (first-match still applies per hook). They also gate [event export](/observability/event-export.md) sinks that use **`tag_required`** or similar filters.

#### YAML equivalent

```yaml
- type: set_tag
  value: suspicious          # key only → true
- type: set_tag
  value: tier=vip            # string value
```

Request- and response-hook rules both support **`set_tag`**. See [Request-hook actions](/policy-routing/rules-and-actions.md#request-hook-actions) and [Response-hook actions](/policy-routing/rules-and-actions.md#response-hook-actions).

#### Example

```rhai
if question_qname(txn).ends_with(".corp.example.") {
    txn.set_tag("corp", true);
    txn.set_tag("tier", "internal");
}
```

</div>

---

## Related topics

- [Hooks and phases](/rhai/hooks-and-phases.md) — [request hook](/rhai/hooks-and-phases.md#request-hook) vs [response hook](/rhai/hooks-and-phases.md#response-hook), pairing scripts, phase-guard table
- [Rules and actions — Request and response hooks](/policy-routing/rules-and-actions.md#request-and-response-hooks) — `hook: request` / `hook: response`, pipeline placement
- [Rules and actions](/policy-routing/rules-and-actions.md) — selectors, action order, scripted policy
- [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule) — soft vs hard drop and retry
- [Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle) — `set_pool`, `set_retry_pool`, and multi-attempt routing
- [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence) — keyed `sample_percent` and selectors
- [User metrics](/rhai/user-metrics.md) — `metric_inc` registration, export tiers, and `conduit_user_*` naming
- [Data sources and lookups](/rhai/data-sources-and-lookups.md) — `data_sources:` and `table_lookup`
- [Dual-stack forwarding](/guides/dual-stack-forwarding.md) — `forward.sources_*`, pool `sources_*`, allowed-set fallback at Forward
