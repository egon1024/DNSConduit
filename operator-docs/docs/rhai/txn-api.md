---
toc_depth: 3
toc_collapsible: true
---

# Transaction API (`txn`)

**Rhai for rules** ([Rule Rhai](/rhai/rule-rhai.md)) scripts receive a sandboxed **`txn`** object — the **per-query policy** surface on the current [transaction](/glossary/index.md#transaction). Methods set pools, [tags](/glossary/index.md#tags), drop/retry intent, egress overrides, and read question/response metadata. They do **not** edit DNS wire bytes.

This page covers **`txn` only**. Lookups, metrics, logging, and runtime reads are separate host surfaces — see [Host API overview](/rhai/host-api.md).

For which hook each API allows, see [Hooks and phases](/rhai/hooks-and-phases.md#phase-guards) and [Host API overview — How to read method reference pages](/rhai/host-api.md#how-to-read-method-reference-pages).

---

## Client and listener

Read-only facts about how the query arrived — client socket, transport, and listener bind label. Use for per-client or per-listener policy without high-cardinality [user metrics](/rhai/user-metrics.md) labels.

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.client_addr()`](#txnclient_addr) · [`txn.client_ip()`](#txnclient_ip) · [`txn.client_port()`](#txnclient_port) · [`txn.client_protocol()`](#txnclient_protocol) · [`txn.listener()`](#txnlistener)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.client_addr()` {#txnclient_addr}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Full client socket address (`ip:port`).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Behavior

- Returns the client **`SocketAddr`** as a string (for example **`192.0.2.1:53000`**).
- Read-only — does not affect routing or metrics.
- Do **not** use client IP or address strings as [user metric](/rhai/user-metrics.md) label values (high cardinality).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.client_ip()` {#txnclient_ip}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Client IP address only (no port).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.client_port()` {#txnclient_port}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64`

Client UDP/TCP source port.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.client_protocol()` {#txnclient_protocol}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Transport the client used: **`udp`** or **`tcp`**.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.listener()` {#txnlistener}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Configured listener bind address label (empty when unset).

</div>

</div>

---

## Egress

When Conduit sends a query to an upstream [backend](/glossary/index.md#backend), it binds a **local address** on your host — that is **egress**. You declare allowed addresses in **`forward.sources_v4`** / **`forward.sources_v6`** and, optionally, per-pool **`sources_v4`** / **`sources_v6`**.

Use Rhai when egress should depend on the query or on what happened upstream:

| Goal | Request hook | Response hook |
|------|--------------|---------------|
| Same local IP for **every** forward on this query | **`txn.set_source_v4(addr)`** or **`txn.set_source_v6(addr)`** | Not allowed — egress for the current attempt is already fixed |
| A **different** local IP only on the **next retry** | **`txn.set_retry_source_v4(addr)`** (stash for later) | **`txn.set_retry_source_v4(addr)`** + **`txn.request_retry()`** when upstream failed or timed out |

If the script does not set a source, Conduit **round-robin**s among the sources configured for the selected pool.

**Allowed addresses:** the IP you pass must be listed in **`forward.sources_*`** or the pool’s **`sources_*`** for that address family. If it is not, Conduit **still answers the client** — it ignores the override and picks another configured source (same as built-in **`set_source_v4`** in YAML). See [Dual-stack forwarding — Choosing an egress source](/guides/dual-stack-forwarding.md#choosing-an-egress-source).

**Pool and egress are separate:** **`txn.set_pool("premium")`** does not change egress by itself. Set both when you need a specific pool **and** a specific bind address. On one rule, list built-in **`set_pool`** before **`set_source_*`** so Forward checks the address against the pool you intended.

**Example — egress from a lookup table** (request hook):

```rhai
let egress = lookup("egress_map", txn.question().qname);
if egress != "" {
    txn.set_source_v4(egress);
}
```

**Example — different egress only on retry** (response hook after a slow upstream):

```rhai
if txn.last_forward_ms() > 800 {
    txn.set_retry_source_v4("10.0.0.9");
    txn.request_retry();
}
```

Retry-specific sources apply to **one** retry forward, then Conduit returns to the standing source from the request hook (if any). Full lifecycle: [Source selection](/policy-routing/retries-and-transactions.md#source-selection-lifecycle).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.set_source_v4`](#txnset_source_v4addr) · [`txn.set_source_v6`](#txnset_source_v6addr) · [`txn.set_retry_source_v4`](#txnset_retry_source_v4addr) · [`txn.set_retry_source_v6`](#txnset_retry_source_v6addr) · [`txn.clear_retry_source_v4`](#txnclear_retry_source_v4) · [`txn.clear_retry_source_v6`](#txnclear_retry_source_v6)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.set_source_v4` {#txnset_source_v4addr}

<div class="txn-api-brief" markdown="1">

Request hook only · `addr`: string (IPv4) · no return; script error on response hook or invalid address

Sets the standing local IPv4 bind for every forward attempt on this transaction.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) only

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv4 address (for example `"10.0.0.5"`) |
| *return* | — | No return value on success |

Returns a **script error** (phase guard or parse failure) when called on the wrong hook or with an invalid address — see **Behavior**.

<p class="txn-api-summary" markdown="1">

**Summary:** Sets the standing local IPv4 bind for every forward attempt on this transaction (request hook only). Conduit checks the address against pool/global allowed sources at Forward — disallowed values fail open to round-robin.

</p>

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
let egress = lookup("egress_map", txn.question().qname);
if egress != "" {
    txn.set_source_v4(egress);
}
```

Fixed literal (repository fixture pattern — address must be in `forward.sources_v4` or pool `sources_v4` for the override to take effect):

```rhai
txn.set_source_v4("127.0.0.1");
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_source_v6` {#txnset_source_v6addr}

<div class="txn-api-brief" markdown="1">

Request hook only · `addr`: string (IPv6) · no return; script error on response hook or invalid address

Sets the standing local IPv6 bind for every forward attempt on this transaction.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) only

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv6 address (for example `"2001:db8::1"` or `"::1"`) |
| *return* | — | No return value on success |

Returns a **script error** (phase guard or parse failure) when called on the wrong hook or with an invalid address — see **Behavior**.

<p class="txn-api-summary" markdown="1">

**Summary:** Sets the standing local IPv6 bind for every forward attempt on this transaction (request hook only). Same allowed-source and fail-open rules as `set_source_v4`, for IPv6 backends.

</p>

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
if txn.question().qname.ends_with(".v6.example.") {
    txn.set_pool("v6-upstream");
    txn.set_source_v6("::1");
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_retry_source_v4` {#txnset_retry_source_v4addr}

<div class="txn-api-brief" markdown="1">

Request + response hook · `addr`: string (IPv4) · one-shot retry egress; pair with `request_retry`

Stashes a one-shot IPv4 egress override consumed on the next retry forward only.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv4 address (for example `"10.0.0.5"`) |
| *return* | — | No return value on success |

Returns a **script error** on invalid address parse failure.

<p class="txn-api-summary" markdown="1">

**Summary:** Stashes a one-shot IPv4 egress used only on the next retry forward (`attempt_count > 1`). Does not trigger retry — pair with `request_retry` on the response hook when needed.

</p>

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
if txn.response_rcode() == Rcode::SERVFAIL {
    txn.set_retry_source_v4("10.0.0.5");
    txn.request_retry();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_retry_source_v6` {#txnset_retry_source_v6addr}

<div class="txn-api-brief" markdown="1">

Request + response hook · `addr`: string (IPv6) · one-shot retry egress; pair with `request_retry`

Stashes a one-shot IPv6 egress override consumed on the next retry forward only.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `addr` | string | IPv6 address (for example `"2001:db8::1"` or `"::1"`) |
| *return* | — | No return value on success |

Returns a **script error** on invalid address parse failure.

<p class="txn-api-summary" markdown="1">

**Summary:** Stashes a one-shot IPv6 egress for the next retry forward only. Independent of the v4 retry stash; only the family matching the backend is used.

</p>

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

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry_source_v4` {#txnclear_retry_source_v4}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · clears stashed retry IPv4 override

Clears a stashed retry IPv4 egress override without affecting standing overrides.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. No return value.

<p class="txn-api-summary" markdown="1">

**Summary:** Clears `retry_source_override_v4` without changing standing `source_override_v4` from `set_source_v4`.

</p>

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

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry_source_v6` {#txnclear_retry_source_v6}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · clears stashed retry IPv6 override

Clears a stashed retry IPv6 egress override without affecting standing overrides.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. No return value.

<p class="txn-api-summary" markdown="1">

**Summary:** Clears `retry_source_override_v6` without changing standing `source_override_v6` from `set_source_v6`.

</p>

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

</div>

---

## Timing and clocks

Wall-clock time and forward [attempt](/glossary/index.md#retry) count on the current [transaction](/glossary/index.md#transaction). Custom policy counters live on the separate **`metrics`** scope object — see [User metrics](/rhai/user-metrics.md).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.elapsed_ms()`](#txnelapsed_ms) · [`txn.get_attempt_count()`](#txnget_attempt_count) · [`txn.last_forward_ms()`](#txnlast_forward_ms) · [`txn.now_unix()`](#txnnow_unix) · [`txn.utc_hour()`](#txnutc_hour) · [`txn.utc_weekday()`](#txnutc_weekday)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.elapsed_ms` {#txnelapsed_ms}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64` (ms since transaction start)

Returns milliseconds since the transaction started.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns **`i64`** — milliseconds elapsed since the transaction started.

<p class="txn-api-summary" markdown="1">

**Summary:** Wall-clock milliseconds since the transaction started — includes request rules, Route, Forward, wait, and any prior response-rule passes. Not upstream RTT alone.

</p>

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
    metrics.inc("slow_login", 1);
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.last_forward_ms` {#txnlast_forward_ms}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64` (latest upstream RTT; `0` on request hook)

Returns upstream RTT in ms for the latest forward attempt (`0` on request hook).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns **`i64`** — milliseconds for the **most recent** upstream forward attempt on this transaction.

<p class="txn-api-summary" markdown="1">

**Summary:** Upstream send→answer-or-timeout time for the most recent forward attempt. Always `0` on the request hook; overwritten on each retry attempt on the response hook.

</p>

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

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.get_attempt_count` {#txnget_attempt_count}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64` (forward attempt count at hook entry)

Returns how many forward attempts have started when the hook runs.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns **`i64`** — the transaction’s forward **attempt count** at hook entry.

<p class="txn-api-summary" markdown="1">

**Summary:** Forward attempt count at hook entry: `0` on request hook, `1` after the first Route/Forward round trip, and so on for retries.

</p>

#### Behavior

- Reflects how many times [Route](/concepts/architecture-and-packet-path.md#route) has selected a pool and recorded a forward attempt on this transaction. Increment happens at Route, **before** [Forward](/concepts/architecture-and-packet-path.md#forward) for that attempt.
- **Request hook:** always **`0`** — no Route has run yet.
- **First response hook** (after one upstream round trip): **`1`**.
- **Second response hook** (after one retry): **`2`**, and so on.
- Use on the response hook to branch on first vs subsequent upstream outcomes (for example only retry once, or different metrics per attempt). Pair with [Retries and transactions](/policy-routing/retries-and-transactions.md) and [`txn.request_retry`](/rhai/txn-api.md#txnrequest_retry).
- Read-only — does not change the transaction.
- The value matches **`attempt_count`** on [event export](/observability/event-export.md) extra fields when that field is enabled.

#### YAML equivalent

None. Declarative rules do not expose attempt count as a selector today.

#### Example

Response script — act only on the first upstream failure:

```rhai
if txn.get_attempt_count() == 1 && txn.response_rcode() == Rcode::SERVFAIL {
    txn.request_retry();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.now_unix()` {#txnnow_unix}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64`

UTC Unix timestamp (seconds) when the transaction started.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Wall-clock **UTC** seconds since epoch, captured when Conduit accepted the client query.
- Stable for the lifetime of the transaction (does not advance on retries).
- Use with **`txn.utc_hour()`** / **`txn.utc_weekday()`** for maintenance-window style policy.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.utc_hour()` {#txnutc_hour}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64` (0–23 UTC)

Hour-of-day in UTC when the transaction started.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.utc_weekday()` {#txnutc_weekday}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64` (1–7 UTC)

ISO weekday in UTC when the transaction started (**1** = Monday, **7** = Sunday).

</div>

</div>

---

## Introspection

Read-only identifiers for correlating script behavior with [tracing](/observability/tracing.md), logs, and config reloads.

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.txn_id()`](#txntxn_id) · [`txn.config_generation()`](#txnconfig_generation) · [`txn.rule_name()`](#txnrule_name)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.txn_id()` {#txntxn_id}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64`

Internal transaction id (same value as **`txn_id`** in debug logs and **`conduitctl trace`**).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Per-worker sequence — not globally unique across the cluster.
- Read-only. Do **not** use as a [user metric](/rhai/user-metrics.md) label (disallowed key **`txn_id`**).
- Pair with **`log.info`** / **`log.warn`** when debugging policy on a single query — see [Script logging](/rhai/script-logging.md).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.config_generation()` {#txnconfig_generation}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `i64`

Config [snapshot generation](/control-plane/configuration-model.md) active when this transaction started.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Matches [`conduit_config_generation`](/observability/built-in-metrics.md#conduit_config_generation) for the snapshot this query runs under.
- Useful for canary rules after reload — branch policy when generation crosses a threshold.

</div>

</div>

---

## Outcomes { #outcomes }

Drop, retry, and response **RCODE** metadata on the [transaction](/glossary/index.md#transaction). **Soft** calls (`drop_query`, `request_retry`) set intent that Conduit resolves **after** the rest of the script (and any later built-in actions on the same rule) finish. **Hard** calls (`drop_query_now`, `request_retry_now`) stop the script immediately — no further Rhai runs on that rule pass. See [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule) for precedence (drop beats retry; soft drop blocks hard retry unless **`clear_drop`** ran earlier on the rule).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.clear_drop`](#txnclear_drop) · [`txn.clear_retry`](#txnclear_retry) · [`txn.drop_query`](#txndrop_query) · [`txn.drop_query_now`](#txndrop_query_now) · [`txn.request_retry`](#txnrequest_retry) · [`txn.request_retry_now`](#txnrequest_retry_now) · [`txn.set_rcode`](#txnset_rcodename)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.drop_query` {#txndrop_query}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · no return

Sets soft-drop intent — resolved at end of rule; later script lines still run.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Soft drop — the query stops at the end of this rule pass with **no** DNS reply if drop intent is still set. Does not stop the script immediately.

</p>

#### Behavior

- Sets **soft-drop** intent on the [transaction](/glossary/index.md#transaction) (same as built-in **`drop`**).
- Later lines in the **same script** still run. Built-in actions **after** a **`rhai`** step on the same rule also still run when the script does not hard-stop.
- Conduit resolves outcome **once** after all actions on the rule: if soft drop is still set, the query **drops**; otherwise policy continues. See [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule).
- If both soft drop and soft retry are set at the end of a response rule, **drop wins**.
- On the **request hook**, drop prevents upstream [Forward](/concepts/architecture-and-packet-path.md#forward) for this transaction. On the **response hook**, drop prevents [Send](/concepts/architecture-and-packet-path.md#send) to the client.
- Use **`txn.clear_drop()`** later on the **same rule** to cancel soft-drop intent from an earlier **`drop`** / **`txn.drop_query()`** call.
- Pair with **`txn.drop_query_now()`** when policy should stop the script immediately instead — see **`txn.drop_query_now`**.

#### YAML equivalent

```yaml
- type: drop
```

#### Example

Request hook — custom metric and soft-drop a blocked name (walkthrough: [Rhai policy — Blocklist drop](/guides/rhai-policy.md#example-1-blocklist-drop-request-hook); repository fixture `blocklist.rhai` / `with-rhai-blocklist.yaml`):

```rhai
if lookup("blocklist", txn.question().qname) == "block" {
    metrics.inc("block_hits", 1);
    txn.drop_query();
}
```

On a silent request drop, **`metrics.inc`** is the usual choice for block counters; **`set_tag`** can still gate [event export](/observability/event-export.md) **`query`** frames when sinks use **`tag_required`**. See [User metrics](/rhai/user-metrics.md).

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.drop_query_now` {#txndrop_query_now}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · no return

Hard drop — stops the script immediately; query drops with no DNS reply.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Hard drop — same outcome as soft drop (no reply) but **stops the script immediately**; no further Rhai or later built-in actions on this rule run.

</p>

#### Behavior

- Same client-visible outcome as **`txn.drop_query()`** — no DNS answer — but returns **`DropNow`** from the script runner so **no further lines** in this script execute.
- When a rule lists built-in actions **before** **`rhai`**, those run first; a **`drop_query_now()`** inside the script still prevents any actions **after** the **`rhai`** step on that rule.
- Does **not** implicitly clear soft retry or pool stashes — it ends the rule pass. Use on both hooks when policy is final on this rule.
- Prefer **`txn.drop_query()`** when later script logic or a later built-in action on the same rule should still run (for example metrics or tag updates before drop).

#### YAML equivalent

```yaml
- type: drop_now
```

#### Example

Request hook — immediate drop when lookup marks the qname as blocked:

```rhai
if lookup("blocklist", txn.question().qname) == "block" {
    txn.drop_query_now();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.clear_drop` {#txnclear_drop}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · no return

Clears soft-drop intent set earlier on this rule pass.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Cancels soft-drop intent from an earlier **`drop`** / **`txn.drop_query()`** on the same rule — use before **`request_retry_now`** when retry should win over an earlier soft drop.

</p>

#### Behavior

- Clears **soft-drop** intent on the [transaction](/glossary/index.md#transaction) for this rule evaluation (same as built-in **`clear_drop`**).
- Only affects intent set **on this rule pass** — not drops already committed by an earlier matching rule.
- Typical use: an earlier action (built-in or Rhai) set soft drop, but later script logic decides to **retry** instead. Call **`txn.clear_drop()`** before **`txn.request_retry()`** / **`txn.request_retry_now()`** — hard retry does **not** clear soft drop automatically. See [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule).
- Manual lab with ordered actions: `tests/manual/ordered-rule-actions.md` and `tests/manual/config/09-ordered-actions.yaml` in the repository.

#### YAML equivalent

```yaml
- type: clear_drop
```

#### Example

Response script — cancel an earlier soft drop and retry instead:

```rhai
if txn.response_rcode() == Rcode::SERVFAIL {
    txn.clear_drop();
    txn.request_retry();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.request_retry` {#txnrequest_retry}

<div class="txn-api-brief" markdown="1">

Response hook only · no args · no return; no effect on request hook

Soft retry — resolved at end of rule; re-enters Route when still set.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Response hook](/rhai/hooks-and-phases.md#response-hook) only

On the [request hook](/rhai/hooks-and-phases.md#request-hook), calls are ignored (no effect, no error).

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Soft retry — re-enter [Route](/concepts/architecture-and-packet-path.md#route) after this rule if retry intent is still set and the query is not dropped. Does not stop the script immediately.

</p>

#### Behavior

- Sets **soft-retry** intent (same as built-in **`retry`**). Conduit resolves it **after** the rest of the script and any later built-in actions on the rule.
- Re-enters [Route](/concepts/architecture-and-packet-path.md#route) for another [Forward](/concepts/architecture-and-packet-path.md#forward) attempt when retry wins at end of rule — subject to orchestrator caps ([Retries and transactions](/policy-routing/retries-and-transactions.md)).
- Does **not** pick a pool by itself — uses **`selected_pool`** unless **`retry_pool`** is set ([Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle)). Pair with **`txn.set_retry_pool`** when failover should use a different pool.
- Blocked when soft drop is still set at end of rule — drop wins. Does **not** clear soft drop.
- **`txn.request_retry_now()`** stops the script immediately instead; use when no further script lines should run.
- Use **`txn.clear_retry()`** to cancel soft-retry intent from an earlier call on the same rule.

#### YAML equivalent

```yaml
- type: retry
```

Response hook only — invalid on `hook: request` (config validation fails).

Prefer declarative **`retry`** when **`retry_pool`** is already set on the request hook — see [Retries and transactions — Declarative examples](/policy-routing/retries-and-transactions.md#declarative-examples). Use Rhai when the response rule needs logic beyond an **`rcode`** selector.

#### Example

Response hook — retry after slow **SERVFAIL** (Rhai adds a latency gate declarative selectors cannot express):

```rhai
if txn.response_rcode() == Rcode::SERVFAIL && txn.last_forward_ms() > 2000 {
    txn.set_retry_pool("secondary");
    txn.request_retry();
}
```

When **`retry_pool`** is already stashed on the request hook, the response script can call **`txn.request_retry()`** alone. Repository fixture `servfail-retry.rhai` / `with-rhai-servfail-retry.yaml` exercises API parity with built-in **`set_retry_pool`** + **`retry`**.

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.request_retry_now` {#txnrequest_retry_now}

<div class="txn-api-brief" markdown="1">

Response hook only · no args · no return; no effect on request hook

Hard retry — stops the script immediately and re-enters Route (unless soft drop blocks).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Response hook](/rhai/hooks-and-phases.md#response-hook) only

On the [request hook](/rhai/hooks-and-phases.md#request-hook), calls are ignored (no effect, no error).

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Hard retry — same retry outcome as **`request_retry`** when allowed, but **stops the script immediately** after setting intent.

</p>

#### Behavior

- Returns **`RetryNow`** from the script runner — no further lines in this script run, and no built-in actions **after** the **`rhai`** step on this rule.
- If soft drop is still set on the [transaction](/glossary/index.md#transaction) at retry time, outcome is **drop**, not retry — even after **`request_retry_now()`**. Call **`txn.clear_drop()`** first when retry should override an earlier soft drop on the same rule.
- Does **not** clear soft drop by itself. See [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule).
- Pool selection follows the same rules as **`txn.request_retry()`** — pair with **`txn.set_retry_pool`** / **`txn.set_pool`** as needed.

#### YAML equivalent

```yaml
- type: retry_now
```

Response hook only — invalid on `hook: request`.

#### Example

Response script — fail over immediately on timeout with no further bookkeeping:

```rhai
if txn.last_forward_ms() > 800 {
    txn.set_retry_pool("secondary");
    txn.request_retry_now();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry` {#txnclear_retry}

<div class="txn-api-brief" markdown="1">

Response hook only · no args · no return; no effect on request hook

Clears soft-retry intent set earlier on this rule pass.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Response hook](/rhai/hooks-and-phases.md#response-hook) only

On the [request hook](/rhai/hooks-and-phases.md#request-hook), calls are ignored (no effect, no error).

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Cancels soft-retry intent from an earlier **`retry`** / **`txn.request_retry()`** on the same rule — the query continues toward Send unless drop intent wins.

</p>

#### Behavior

- Clears **soft-retry** intent for this rule evaluation (same as built-in **`clear_retry`**).
- Does **not** clear **`retry_pool`** or standing pool choice — use **`txn.clear_retry_pool()`** for the pool stash ([Routing](/rhai/txn-api.md#routing)).
- Does **not** undo a retry already committed by **`request_retry_now()`** on an earlier line in the same script (hard retry already stopped the script).
- Typical use: conditional retry — an earlier branch called **`txn.request_retry()`**, but later logic decides to accept the answer instead.

#### YAML equivalent

```yaml
- type: clear_retry
```

Response hook only — invalid on `hook: request`.

#### Example

```rhai
if txn.get_attempt_count() >= 2 {
    txn.clear_retry();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_rcode` {#txnset_rcodename}

<div class="txn-api-brief" markdown="1">

Request + response hook · `rcode`: [`Rcode`](#rcode) or string · no return

Sets response RCODE metadata on the transaction before Send.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

(YAML **`set_rcode`** is [response hook](/rhai/hooks-and-phases.md#response-hook) only — config validation rejects it on `hook: request`.)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `rcode` | [`Rcode`](#rcode) or string | Prefer **`Rcode::SERVFAIL`**; strings such as **`"SERVFAIL"`** still accepted (case-insensitive) |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Sets the **RCODE** Conduit attaches to the response metadata for this transaction — for example before [Send](/concepts/architecture-and-packet-path.md#send) when policy accepts an upstream answer.

</p>

#### Behavior

- Writes **`rcode`** on the [transaction](/glossary/index.md#transaction). Downstream phases use this when building the client response.
- Accepts a [`Rcode`](#rcode) value or a case-insensitive string name / **`RCODE{n}`** alias. Unrecognized strings map to **`SERVFAIL`** (fail-safe default).
- Does **not** by itself trigger drop or retry — pair with outcome methods when policy should fail over instead of accepting.
- On the **response hook**, commonly used after inspecting **`txn.response_rcode()`** when rewriting metadata for the client. On the **request hook**, rare — prefer response-hook scripts unless you need to pre-stage metadata before upstream.
- Within one rule, later **`set_rcode`** (built-in or Rhai) overrides an earlier value on the same hook pass.

#### YAML equivalent

```yaml
- type: set_rcode
  value: SERVFAIL
```

Response hook only in config.

#### Example

Response script — normalize a policy outcome before Send:

```rhai
if txn.response_rcode() == Rcode::SERVFAIL && txn.get_attempt_count() >= 3 {
    txn.set_rcode(Rcode::REFUSED);
}
```

</div>

</div>

---

## Query and response { #query-and-response }

Read-only access to the **client question** and, on the [response hook](/rhai/hooks-and-phases.md#response-hook), the **upstream outcome metadata** Conduit recorded after [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response). These APIs expose qname, typed DNS wire enums ([`RecordType`](#recordtype), [`QueryClass`](#queryclass), [`DnsOpcode`](#dnsopcode), [`EdnsOptionCode`](#ednsoptioncode)), message **ID**, and [`Rcode`](#rcode) — not full answer records or wire bytes.

On the **request hook**, only the question is meaningful — upstream has not answered yet. On the **response hook**, the question is unchanged and **`txn.response()`** / **`txn.response_rcode()`** reflect the outcome of the **current** forward attempt (including timeout or pool exhaustion, which Conduit typically records as **`SERVFAIL`**). See [Hooks and phases — phase guards](/rhai/hooks-and-phases.md#phase-guards).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.question()`](#txnquestion) · [`txn.response()`](#txnresponse) · [`txn.response_rcode()`](#txnresponse_rcode) · [`txn.selected_pool()`](#txnselected_pool) · [`txn.selected_backend()`](#txnselected_backend) · [`txn.selected_backend_name()`](#txnselected_backend_name) · [`txn.response_truncated()`](#txnresponse_truncated) · [`txn.response_answer_count()`](#txnresponse_answer_count) · **Types:** [`RecordType`](#recordtype) · [`Rcode`](#rcode) · [`QueryClass`](#queryclass) · [`DnsOpcode`](#dnsopcode) · [`EdnsOptionCode`](#ednsoptioncode)

</p>

<div class="txn-api-entry" markdown="1">

### `RecordType` {#recordtype}

<div class="txn-api-brief" markdown="1">

Rule Rhai global · static module · DNS QTYPE enum

Named constants and **`TYPE{n}`** aliases for every IANA-assigned RR type (plus Conduit-specific **`ANAME`**); use for **`qtype`** comparisons and **`RecordType::from_number(n)`** for arbitrary wire numbers.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Availability

Registered on the Rule Rhai engine — available in every **`type: rhai`** script without import.

#### Constants

Each known type appears twice in the **`RecordType`** module:

| Form | Example | Wire number |
|------|---------|-------------|
| Name | **`RecordType::A`**, **`RecordType::HTTPS`** | 1, 65, … |
| **`TYPE{n}`** alias | **`RecordType::TYPE1`**, **`RecordType::TYPE65`** | same |

Known names follow the IANA DNS RR type registry (for example **`A`**, **`AAAA`**, **`CNAME`**, **`MX`**, **`NS`**, **`PTR`**, **`SOA`**, **`SRV`**, **`TXT`**, **`HTTPS`**, **`SVCB`**, **`DNSKEY`**, **`DS`**, **`TLSA`**, **`CAA`**, **`ANY`**, **`ANAME`**). Unknown numbers are still valid via **`RecordType::from_number(n)`**; **`name()`** on such values returns **`TYPE{n}`**.

#### Methods on values

| Method | Returns | Notes |
|--------|---------|-------|
| **`number()`** | **`i64`** | IANA type number (0–65535) |
| **`name()`** | string | **`A`**, **`HTTPS`**, or **`TYPE{n}`** for unknown types |
| **`==`**, **`!=`** | bool | Compare wire numbers — **`RecordType::A == RecordType::TYPE1`** is **`true`** |

#### Constructor

| Call | Notes |
|------|-------|
| **`RecordType::from_number(n)`** | Build from wire number; error if **`n`** ∉ 0…65535 |

#### Example

```rhai
let q = txn.question();
if q.qtype == RecordType::AAAA {
    txn.set_pool("v6-only");
} else if q.qtype == RecordType::from_number(99) {
    // custom TYPE99 policy
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `Rcode` {#rcode}

<div class="txn-api-brief" markdown="1">

Rule Rhai global · static module · DNS RCODE enum

Response codes for **`txn.response().rcode`**, **`txn.response_rcode()`**, and **`txn.set_rcode(...)`** — every IANA-assigned RCODE with a name (`SERVFAIL`, `NXDOMAIN`, `DSOTYPENI`, …) and matching **`RCODE{n}`** alias.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Constants

| Form | Example |
|------|---------|
| Name | **`Rcode::NOERROR`**, **`Rcode::SERVFAIL`**, **`Rcode::NXDOMAIN`** |
| **`RCODE{n}`** | **`Rcode::RCODE0`**, **`Rcode::RCODE2`** |

Also **`Rcode::BADSIG`** as an alias for wire **16** (same as **`Rcode::BADVERS`** / **`Rcode::RCODE16`**). Unknown codes use **`Rcode::from_number(n)`**; **`name()`** returns **`RCODE{n}`**.

#### Example

```rhai
if txn.response_rcode() == Rcode::SERVFAIL {
    txn.request_retry();
}
txn.set_rcode(Rcode::REFUSED);
```

Same **`number()`**, **`name()`**, **`==`**, and **`from_number(n)`** pattern as [`RecordType`](#recordtype).

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `QueryClass` {#queryclass}

<div class="txn-api-brief" markdown="1">

Rule Rhai global · static module · DNS QCLASS enum

Query class on **`txn.question().qclass`** — all IANA scalar class assignments (**`IN`**, **`CH`**, **`HS`**, **`NONE`**, **`ANY`**, …), plus **`CLASS{n}`** aliases.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Example

```rhai
if txn.question().qclass == QueryClass::IN {
    // typical Internet class
}
```

Same **`number()`**, **`name()`**, **`==`**, and **`from_number(n)`** pattern as [`RecordType`](#recordtype).

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `DnsOpcode` {#dnsopcode}

<div class="txn-api-brief" markdown="1">

Rule Rhai global · static module · DNS opcode enum

Message opcode from the query header — **`txn.question().opcode`**. IANA-assigned opcodes include **`QUERY`**, **`STATUS`**, **`NOTIFY`**, **`UPDATE`**, obsolete **`IQUERY`**, and **`DNS_STATEFUL_OPERATIONS`** (alias **`DSO`**), each with an **`OPCODE{n}`** alias.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Example

```rhai
if txn.question().opcode != DnsOpcode::QUERY {
    txn.drop_query();
}
```

Same **`number()`**, **`name()`**, **`==`**, and **`from_number(n)`** pattern as [`RecordType`](#recordtype).

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `EdnsOptionCode` {#ednsoptioncode}

<div class="txn-api-brief" markdown="1">

Rule Rhai global · static module · EDNS(0) option code enum

Option codes present on the client OPT record — **`txn.question().edns_options`** is an array of **`EdnsOptionCode`** values (empty when the query has no EDNS). Named constants cover every IANA-assigned EDNS option code, including **`COOKIE`**, **`EDNS_CLIENT_SUBNET`** (alias **`CLIENT_SUBNET`**), **`PADDING`**, **`EXTENDED_DNS_ERROR`** (alias **`EDE`**), **`REPORT_CHANNEL`**, and Cisco **`UMBRELLA_IDENT`** (alias **`UMBRELLA`**), each with a **`CODE{n}`** alias.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Example

```rhai
let q = txn.question();
for opt in q.edns_options {
    if opt == EdnsOptionCode::COOKIE {
        // client sent DNS cookies
    } else if opt == EdnsOptionCode::UMBRELLA {
        // Cisco Umbrella network-device identification (wire 20292)
    } else if opt == EdnsOptionCode::REPORT_CHANNEL {
        // RFC 9567 report channel (wire 18)
    }
}
```

Same **`number()`**, **`name()`**, **`==`**, and **`from_number(n)`** pattern as [`RecordType`](#recordtype).

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.question()` {#txnquestion}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns map

Returns a map of client question fields: `qname`, `qtype`, `qclass`, `opcode`, `edns_options`, and DNS message `id` (typed enums where noted below).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | map | Question metadata (see **Behavior**) |

There is no YAML equivalent.

<p class="txn-api-summary" markdown="1">

**Summary:** Structured read of the client question — qname, typed wire enums, and DNS message ID — as a Rhai map.

</p>

#### Behavior

- Always available on both hooks. On the **response hook**, values describe the **original client question**, not upstream answer data.
- Map keys (each present only when Conduit has a value):
  - **`qname`** — string, same as **`txn.question().qname`**
  - **`qtype`** — [`RecordType`](#recordtype)
  - **`qclass`** — [`QueryClass`](#queryclass)
  - **`opcode`** — [`DnsOpcode`](#dnsopcode)
  - **`edns_options`** — array of [`EdnsOptionCode`](#ednsoptioncode) (omitted when empty — no EDNS on the query)
  - **`id`** — integer DNS message ID from the client query (16-bit; exposed as Rhai **`i64`**)
- Each wire enum uses its static module for constants (name + numeric alias, e.g. **`RecordType::A`** / **`RecordType::TYPE1`**, **`QueryClass::IN`** / **`QueryClass::CLASS1`**). Compare with **`==`**, or use **`from_number(n)`** for arbitrary wire values; **`.name()`** returns the selector-friendly string.
- Missing fields are **omitted** from the map rather than set to empty values — use **`txn.question().qname`** or check map membership when you need a default.
- Does **not** include client address, EDNS options, or answer records — only parsed question metadata from [Parse](/concepts/architecture-and-packet-path.md#parse).
- Use **`txn.question().qname`** when you only need the name; use **`txn.question()`** when **`qtype`** or **`id`** matter (for example metrics labels or TYPE-specific policy).

#### Example

Request hook — route HTTPS queries differently:

```rhai
let q = txn.question();
if q.qtype == RecordType::HTTPS || q.qtype == RecordType::TYPE65 {
    txn.set_pool("doh-helper");
} else if q.qname.ends_with(".slow.example.") {
    txn.set_pool("bulk");
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.response()` {#txnresponse}

<div class="txn-api-brief" markdown="1">

Response hook only · no args · returns map; script error on request hook

Returns upstream outcome metadata for the current forward attempt (`rcode`, routing path, optional wire-derived counts, question fields).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Response hook](/rhai/hooks-and-phases.md#response-hook) only

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | map | Response metadata for this attempt (see **Behavior**) |

There is no YAML equivalent.

<p class="txn-api-summary" markdown="1">

**Summary:** Map view of upstream outcome metadata after the latest forward — **`rcode`**, **`pool`** / **`backend`** for this attempt, optional wire-derived counts when scripts need them, plus question fields for context. Not available on the request hook.

</p>

#### Behavior

- **Response hook only.** On the **request hook**, calling **`txn.response()`** returns a **script error** (`response() is not available in request phase`). Conduit logs the error, skips further script effects for that hook invocation, and continues the pipeline ([Sandbox limits](/rhai/sandbox-limits.md) — fail-open). Repository fixture: `bad-phase.rhai` / `with-rhai-bad-phase.yaml`.
- Reflects the outcome Conduit recorded for the **current** forward attempt — after timeout, connection failure, or an upstream DNS response. Conduit often sets **`SERVFAIL`** for timeout and pool exhaustion before your script runs; see [Retries and transactions](/policy-routing/retries-and-transactions.md).
- Map keys (each present only when available):
  - **`rcode`** — [`Rcode`](#rcode) for this forward attempt
  - **`pool`**, **`backend`** — selected pool name and upstream backend **socket address** for **this** forward attempt (also available via [`txn.selected_pool()`](#txnselected_pool) / [`txn.selected_backend()`](#txnselected_backend))
  - **`backend_name`** — backend **logical label** (configured `name` when set, else address) for this attempt; matches the name-when-set identity in metrics/logs/traces/event filters (also via [`txn.selected_backend_name()`](#txnselected_backend_name))
  - **`answer_count`**, **`authority_count`**, **`additional_count`**, **`truncated`**, **`authoritative`** — present only when a response-hook script references wire-derived fields at compile time (see below)
  - **`qname`**, **`qtype`**, **`qclass`**, **`opcode`**, **`edns_options`** — same as **`txn.question()`**
- **Compile-time gating:** Conduit scans response-hook Rhai sources at snapshot compile. If **no** script references wire-derived fields (`truncated`, `answer_count`, etc.), the forward stage skips parsing upstream response sections — **`rcode`** is still extracted. When any script needs wire metadata, parsing is enabled for **all** queries on that snapshot.
- Dedicated accessors — [`txn.response_truncated()`](#txnresponse_truncated), [`txn.response_answer_count()`](#txnresponse_answer_count), etc. — return defaults when wire metadata was not parsed (**`-1`** for counts, **`false`** for booleans).
- Does **not** expose answer RRs, TTLs, or wire bytes — only transaction-level metadata scripts use for policy.
- On a retried query, the map reflects **this attempt only** — compare with **`txn.get_attempt_count()`** when policy depends on retry generation.
- For simple **`rcode`** branching, **`txn.response_rcode()`** is usually clearer and is safe to call on both hooks (returns **`()`** on the request hook when no rcode is available).

#### Example

Response hook — branch on rcode and attempt count:

```rhai
let resp = txn.response();
if resp.rcode == Rcode::SERVFAIL && txn.get_attempt_count() == 1 {
    txn.set_retry_pool("secondary");
    txn.request_retry();
}
```

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.selected_pool()` {#txnselected_pool}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Pool name selected for the current forward attempt (empty when unset).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.selected_backend()` {#txnselected_backend}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Upstream backend **socket address** for the current forward attempt (empty when unset).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.selected_backend_name()` {#txnselected_backend_name}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns `string`

Upstream backend **logical label** for the current forward attempt: the configured backend `name` when set, otherwise the socket address. This is the same name-when-set identity used in [metrics](/observability/built-in-metrics.md), logs, traces, and event-sink `backend` filters. Empty when no backend is selected.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.response_truncated()` {#txnresponse_truncated}

<div class="txn-api-brief" markdown="1">

Response hook · no args · returns `bool`

Whether upstream response had **TC=1** (requires compile-time wire-meta gating).

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.response_answer_count()` {#txnresponse_answer_count}

<div class="txn-api-brief" markdown="1">

Response hook · no args · returns `i64`

Answer section count from upstream wire, or **`-1`** when wire metadata was not parsed.

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.response_rcode()` {#txnresponse_rcode}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns [`Rcode`](#rcode) or `()`

Returns upstream RCODE on the response hook; **`()`** on the request hook when no rcode is available.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | [`Rcode`](#rcode) or **`()`** | Upstream RCODE on response hook; **`()`** on request hook |

There is no YAML equivalent.

<p class="txn-api-summary" markdown="1">

**Summary:** Convenience accessor for upstream **`rcode`** — the most common response-hook branch condition. Returns **`()`** on the request hook (no script error).

</p>

#### Behavior

- On the **response hook**, returns the **RCODE** Conduit recorded for the **current** forward attempt — same value as **`txn.response().rcode`** when that field is present.
- Compare with **`==`** against **`Rcode::SERVFAIL`**, **`Rcode::RCODE2`**, etc.
- Returns **`()`** when:
  - Called on the **request hook** (upstream outcome not available yet) — **does not** raise a phase error (unlike **`txn.response()`**)
  - No **RCODE** is set on the transaction yet for this attempt
- Typical uses: retry/failover on **`SERVFAIL`**, accept or rewrite on **`NOERROR`**, client-facing **`txn.set_rcode`** after inspection. Often paired with **`txn.request_retry()`**, **`txn.set_retry_pool`**, or **`txn.set_rcode`**. See [Outcomes](/rhai/txn-api.md#outcomes) and [Routing](/rhai/txn-api.md#routing).
- Runs **once per response-hook invocation** — on retries, each forward attempt gets a fresh evaluation with the rcode for that attempt.

#### Example

Response hook — conditional retry when upstream was slow (see [Hooks and phases — Pairing](/rhai/hooks-and-phases.md#pairing-request-and-response-scripts)):

```rhai
if txn.response_rcode() == Rcode::SERVFAIL && txn.last_forward_ms() > 2000 {
    txn.set_retry_pool("secondary");
    txn.request_retry();
}
```

Accept only after upstream success:

```rhai
if txn.response_rcode() == Rcode::NOERROR {
    txn.set_tag("upstream_ok", true);
}
```

</div>

</div>

---

## Routing { #routing }

**`set_pool`** and **`set_retry_pool`** choose which [pool](/glossary/index.md#pool) [Route](/concepts/architecture-and-packet-path.md#route) uses. **`clear_pool`** removes a standing pool choice so Route picks the configured **default** pool (the pool named `default`, or the first pool in your config). See [Pool selection lifecycle](/policy-routing/retries-and-transactions.md#pool-selection-lifecycle).

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.clear_pool`](#txnclear_pool) · [`txn.clear_retry_pool`](#txnclear_retry_pool) · [`txn.set_pool`](#txnset_poolname) · [`txn.set_retry_pool`](#txnset_retry_poolname)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.clear_pool` {#txnclear_pool}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · no return

Clears standing pool choice — next Route uses the configured default pool.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. No return value.

<p class="txn-api-summary" markdown="1">

**Summary:** Clears **`selected_pool`** so [Route](/concepts/architecture-and-packet-path.md#route) falls back to the configured default pool (`default` name, or first pool in config).

</p>

#### Behavior

- Sets **`selected_pool`** to unset — same as built-in **`clear_pool`**.
- On the **first** Route (`attempt_count == 0`), Route uses the default pool when **`selected_pool`** is unset.
- On a **retry** Route, Route uses the default pool when **`selected_pool`** is unset and **`retry_pool`** is not set.
- Does **not** clear **`retry_pool`** — use **`txn.clear_retry_pool()`** for that.
- Typical uses: undo an earlier **`set_pool`** on the same rule; CSV lookup miss → default pool without hardcoding the default name; response hook → retry on the default pool instead of the pool that just failed.

#### YAML equivalent

```yaml
- type: clear_pool
```

#### Example

Lookup miss leaves pool at default (request hook):

```rhai
let pool = lookup("routing", txn.question().qname);
if pool != "" {
    txn.set_pool(pool);
} else {
    txn.clear_pool();
}
```

Undo built-in **`set_pool`** when Rhai runs later on the same rule:

```rhai
txn.clear_pool();
```

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `txn.clear_retry_pool` {#txnclear_retry_pool}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · clears `retry_pool` stash only

Clears the `retry_pool` stash from `set_retry_pool` without clearing soft-retry intent.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| *none* | — | |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Clears the `retry_pool` stash from `set_retry_pool` without clearing soft-retry intent or `selected_pool`.

</p>

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
if txn.response_rcode() == Rcode::SERVFAIL {
    txn.clear_retry_pool();
    txn.request_retry();
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_pool` {#txnset_poolname}

<div class="txn-api-brief" markdown="1">

Request + response hook · `name`: string (pool) · no return

Sets `selected_pool` for Route — first forward and later attempts when `retry_pool` is absent.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `name` | string | [Pool](/glossary/index.md#pool) name from config |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Sets `selected_pool` for Route — used on the first forward and on later attempts when `retry_pool` is absent.

</p>

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
let qname = txn.question().qname;
if qname.ends_with(".vip.example.") {
    txn.set_pool("vip");
} else if qname.ends_with(".slow.example.") {
    txn.set_pool("bulk");
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_retry_pool` {#txnset_retry_poolname}

<div class="txn-api-brief" markdown="1">

Request + response hook · `name`: string (pool) · stashes pool for next retry Route

Stashes a pool name consumed once on the next retry Route; does not trigger retry alone.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `name` | string | [Pool](/glossary/index.md#pool) name from config |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Stashes a pool name consumed once on the next retry Route. Does not trigger retry by itself.

</p>

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

Pair with **`retry`** or **`retry_now`** on a response rule to fail over — see [Retry actions](/policy-routing/rules-and-actions.md#retry-actions). Built-in **`set_retry_pool`** on the request or response hook is equivalent when you do not need script logic.

#### Example

Response script — fail over when **SERVFAIL** followed a slow forward:

```rhai
if txn.response_rcode() == Rcode::SERVFAIL && txn.last_forward_ms() > 2000 {
    txn.set_retry_pool("secondary");
    txn.request_retry();
}
```

Request hook — route to **primary** now, stash **secondary** if a later retry occurs (built-in actions on the request rule are equivalent; see `tests/fixtures/config/with-rhai-servfail-retry.yaml`):

```rhai
txn.set_pool("primary");
txn.set_retry_pool("secondary");
```

</div>

</div>

---

## Sampling

Deterministic sampling and cadence gates for scripts — mirror YAML [selectors](/glossary/index.md#selector) on [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence). **Percentage** methods (`sample_percent*`) use a **`0..100`** scale with optional key salt; **cadence** methods (`every_nth_*`) match every Nth query on this worker or process-wide. Use to gate expensive logic, set audit [tags](/glossary/index.md#tags), or combine with declarative rules.

#### YAML selector parity

| YAML selector / field | Rule Rhai equivalent |
|-----------------------|----------------------|
| **`sample_percent`** (no salt) | **`txn.sample_percent(percent)`** |
| **`sample_percent`** + **`key`** | **`txn.sample_percent(percent, key)`** |
| **`sample_percent`** + **`key_from: qname`** | **`txn.sample_percent_for_qname(percent)`** or **`txn.sample_percent(percent, txn.question().qname)`** |
| **`sample_percent`** + **`key_from: rule_name`** | **`txn.sample_percent_for_rule(percent)`** or **`txn.sample_percent(percent, txn.rule_name())`** |
| **`every_nth_worker`** | **`txn.every_nth_worker(n)`** |
| **`every_nth_global`** | **`txn.every_nth_global(n)`** |

**`key_from: sink_name`** applies to event sink filters only — not exposed on Rule Rhai. Prefer a YAML selector on the rule when you only need coarse gating without running script logic on every match.

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.sample_percent(percent)`](#txnsample_percent) · [`txn.sample_percent(percent, key)`](#txnsample_percent) · [`txn.sample_percent_for_qname(percent)`](#txnsample_percent_for_qnamepercent) · [`txn.sample_percent_for_rule(percent)`](#txnsample_percent_for_rulepercent) · [`txn.every_nth_worker(n)`](#txnevery_nth_workern) · [`txn.every_nth_global(n)`](#txnevery_nth_globaln) · [`txn.rule_name()`](#txnrule_name)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.sample_percent` {#txnsample_percent}

<div class="txn-api-brief" markdown="1">

Request + response hook · `percent`: float · optional `key`: string · returns `bool`

Returns whether this transaction falls in the ~`percent`% sample; optional `key` selects an independent bucket.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [Response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

Two overloads share the same name:

| Overload | Parameters | Notes |
|----------|------------|-------|
| Global bucket | `percent`: float | Same hash as YAML **`sample_percent`** with no `key` / `key_from` — transaction id only |
| Keyed bucket | `percent`: float, `key`: string | Same hash as YAML **`sample_percent`** with static **`key:`** |

| Parameter | Type | Notes |
|-----------|------|-------|
| `percent` | float | Target pass rate on **`0..100`** — clamped (`0` never passes; `100` always passes) |
| `key` | string (optional) | Salt string; empty string is treated like no key |
| *return* | `bool` | **`true`** when this transaction is in the sample |

<p class="txn-api-summary" markdown="1">

**Summary:** Deterministic ~`percent`% gate. When the call returns **`true`**, Conduit also sets boolean tag **`sampled`**.

</p>

#### Behavior

- Same hash as rule **`sample_percent`** selectors, tracing **`activation.sample_percent`**, and event-export filters.
- Repeated calls with the same **`percent`** and **`key`** return the same **`bool`** (cached for this hook invocation).
- For **`key_from: qname`** or **`key_from: rule_name`**, prefer the dedicated helpers [`sample_percent_for_qname`](#txnsample_percent_for_qnamepercent) and [`sample_percent_for_rule`](#txnsample_percent_for_rulepercent).
- On internal error acquiring script effects, returns **`false`**.

#### YAML equivalent

```yaml
selectors:
  - type: sample_percent
    value: "10"
```

See also keyed examples under [sample_percent_for_qname](#txnsample_percent_for_qnamepercent) and [sample_percent_for_rule](#txnsample_percent_for_rulepercent).

#### Example

```rhai
if txn.sample_percent(5.0) {
    txn.set_tag("audit", true);
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.sample_percent_for_qname` {#txnsample_percent_for_qnamepercent}

<div class="txn-api-brief" markdown="1">

Request + response hook · `percent`: float · returns `bool`

~`percent`% sample with per-qname salt — matches YAML **`key_from: qname`**.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [Response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `percent` | float | **`0..100`** (clamped) |
| *return* | `bool` | **`false`** when the question has no qname; otherwise same as **`txn.sample_percent(percent, txn.question().qname)`** |

<p class="txn-api-summary" markdown="1">

**Summary:** Keyed **`sample_percent`** using the canonical wire qname as salt. Sets tag **`sampled`** when **`true`**.

</p>

#### YAML equivalent

```yaml
selectors:
  - type: sample_percent
    value: "10"
    key_from: qname
```

#### Example

Repository fixture `sample-audit.rhai`:

```rhai
if txn.sample_percent_for_qname(10.0) {
    txn.set_tag("rhai_sampled", true);
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.sample_percent_for_rule` {#txnsample_percent_for_rulepercent}

<div class="txn-api-brief" markdown="1">

Request + response hook · `percent`: float · returns `bool`

~`percent`% sample salted with this rule's configured **`name`** — matches YAML **`key_from: rule_name`**.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [Response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `percent` | float | **`0..100`** (clamped) |
| *return* | `bool` | Same bucket as **`txn.sample_percent(percent, txn.rule_name())`** |

<p class="txn-api-summary" markdown="1">

**Summary:** Independent ~`percent`% slice per rule name — two rules at the same percentage do not share the same bucket. Sets tag **`sampled`** when **`true`**.

</p>

#### YAML equivalent

```yaml
selectors:
  - type: sample_percent
    value: "10"
    key_from: rule_name
```

#### Example

```rhai
if txn.sample_percent_for_rule(25.0) {
    txn.set_tag("canary", true);
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.every_nth_worker` {#txnevery_nth_workern}

<div class="txn-api-brief" markdown="1">

Request + response hook · `n`: integer · returns `bool`

**`true`** when this worker's transaction id is divisible by **`n`** — matches YAML **`every_nth_worker`**.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [Response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `n` | integer | Must be **`>= 1`** — script error otherwise |
| *return* | `bool` | **`true`** when **`txn_id % n == 0`** (for example **`n: 4`** matches ids **4, 8, 12, …** on each worker) |

<p class="txn-api-summary" markdown="1">

**Summary:** Worker-local cadence gate — same semantics as the **`every_nth_worker`** selector. Read-only; does not set tags.

</p>

#### Behavior

- Uses the worker-local transaction id assigned when Conduit creates the [transaction](/glossary/index.md#transaction).
- Stable for the lifetime of the transaction — same result on request and response hooks (and across [retry](/glossary/index.md#retry) response passes for the same id).
- Does **not** set tag **`sampled`** — unlike **`sample_percent*`**.

#### YAML equivalent

```yaml
selectors:
  - type: every_nth_worker
    value: "4"
```

#### Example

```rhai
if txn.every_nth_worker(4) {
    txn.set_pool("canary");
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.every_nth_global` {#txnevery_nth_globaln}

<div class="txn-api-brief" markdown="1">

Request + response hook · `n`: integer · returns `bool`

**`true`** when the process-wide query index is divisible by **`n`** — matches YAML **`every_nth_global`**.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [Response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `n` | integer | Must be **`>= 1`** — script error otherwise |
| *return* | `bool` | **`true`** when **`global_query_index % n == 0`** |

<p class="txn-api-summary" markdown="1">

**Summary:** Process-wide cadence gate — same semantics as the **`every_nth_global`** selector. Read-only; does not set tags.

</p>

#### Behavior

- Uses the process-wide query index incremented once when each transaction is created (before selector evaluation on rules).
- Coordinates cadence across worker threads — unlike **`every_nth_worker`**, which is scoped per worker.
- Does **not** set tag **`sampled`**.

#### YAML equivalent

```yaml
selectors:
  - type: every_nth_global
    value: "100"
```

#### Example

```rhai
if txn.every_nth_global(100) {
    txn.set_tag("global_canary", true);
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.rule_name` {#txnrule_name}

<div class="txn-api-brief" markdown="1">

Request + response hook · no args · returns string

Returns the configured **`name`** of the rule whose **`rhai`** action is running this script.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [Response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

No arguments. Returns the rule **`name`** string from config (for example **`"audit-canary"`**).

<p class="txn-api-summary" markdown="1">

**Summary:** Read-only rule identity for logging, metrics labels, or custom **`sample_percent(percent, key)`** salts. Prefer [`sample_percent_for_rule`](#txnsample_percent_for_rulepercent) when you want YAML **`key_from: rule_name`** semantics.

</p>

#### YAML equivalent

None — use rule **`name:`** in config. Matches the value baked into **`key_from: rule_name`** selectors at compile time.

#### Example

```rhai
if txn.sample_percent(5.0, txn.rule_name()) {
    metrics.inc("rule_sample_hits", 1);
}
```

</div>

</div>

---

## Tags { #tags }

[Tags](/glossary/index.md#tags) are small key/value labels you attach to a [transaction](/glossary/index.md#transaction) for the rest of its life — including across [retries](/glossary/index.md#retry) and on the [response hook](/rhai/hooks-and-phases.md#response-hook). They do not change routing by themselves; they let later policy, [event export](/observability/event-export.md), and other scripts branch on how the query was classified.

| Goal | Typical hook | API |
|------|--------------|-----|
| Classify the query before upstream | Request | **`txn.set_tag("tier", "vip")`** or **`txn.set_tag("audit", true)`** |
| Act on classification + upstream outcome | Response | **`txn.has_tag("suspicious")`** then metrics, retry, or drop |
| Gate dnstap / event sinks | Request (set tag) | Sink **`tag_required`** in config — see [Event export — Filters](/observability/event-export.md#filters) |
| Remove a label | Either | **`txn.clear_tag("temporary")`** |

Tags set on the **request hook** stay on the transaction when the **response hook** runs (the request hook does not run again on retry). Pair request **`set_tag`** with response **`has_tag`** — see [Hooks and phases — Pairing scripts](/rhai/hooks-and-phases.md#pairing-request-and-response-scripts).

**Example — request classify, response act:**

```rhai
// request hook
if txn.question().qname == "login.suspicious.example." {
    txn.set_tag("suspicious", true);
}

// response hook
if txn.has_tag("suspicious") && txn.last_forward_ms() > 500 {
    metrics.inc("slow_login", 1);
}
```

Boolean tags use **`true`** / **`false`**. String tags store text (`txn.set_tag("tier", "vip")`). YAML built-in **`set_tag`** on the same rule runs before Rhai when listed above the script — the script can read those tags with **`has_tag`** and add more.

<p class="txn-api-index" markdown="1">

**Methods:** [`txn.clear_tag(key)`](#txnclear_tagkey) · [`txn.has_tag(key)`](#txnhas_tagkey) · [`txn.set_tag(key, value)`](#txnset_tagkey-value)

</p>

<div class="txn-api-entry" markdown="1">

### `txn.clear_tag` {#txnclear_tagkey}

<div class="txn-api-brief" markdown="1">

Request + response hook · `key`: string · no return

Removes a tag key from the transaction tag map.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `key` | string | Tag name to remove |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Removes a tag key (boolean or string) from the transaction. Use explicitly instead of `set_tag(key, false)` when you mean removal.

</p>

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

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.has_tag` {#txnhas_tagkey}

<div class="txn-api-brief" markdown="1">

Request + response hook · `key`: string · returns `bool`

Returns whether a tag key is present on the transaction.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `key` | string | Tag name |
| *return* | `bool` | `true` when the tag is present under the rules below |

<p class="txn-api-summary" markdown="1">

**Summary:** Read-only check for a tag: script effects from earlier in the run win, then tags present at hook entry.

</p>

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
        metrics.inc("slow_login", 1);
    }
}
```

</div>

</div>

---

<div class="txn-api-entry" markdown="1">

### `txn.set_tag` {#txnset_tagkey-value}

<div class="txn-api-brief" markdown="1">

Request + response hook · `key`: string, `value`: bool or string · no return

Sets a bool or string tag on the transaction for rules, metrics, and export.

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Hooks

[Request hook](/rhai/hooks-and-phases.md#request-hook) and [response hook](/rhai/hooks-and-phases.md#response-hook)

#### Arguments / return

| Parameter | Type | Notes |
|-----------|------|-------|
| `key` | string | Tag name |
| `value` | bool or string | `true` / `false` for boolean tags; any other value is stored as a string |
| *return* | — | No return value |

<p class="txn-api-summary" markdown="1">

**Summary:** Sets a boolean or string tag that persists for the rest of the transaction, including across retries and into the response hook.

</p>

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
if txn.question().qname.ends_with(".corp.example.") {
    txn.set_tag("corp", true);
    txn.set_tag("tier", "internal");
}
```

</div>

</div>

---

## Related topics

- [Host API overview](/rhai/host-api.md) — five scope objects in every hook
- [Runtime API](/rhai/runtime-api.md) — read-only `runtime.routing` health and routing views
- [Data sources and lookups](/rhai/data-sources-and-lookups.md) — `lookup()` and `data_sources:`
- [User metrics](/rhai/user-metrics.md) — `metrics.inc` / `metrics.inc_labels`
- [Script logging](/rhai/script-logging.md) — `log.info` / `log.warn`
- [Hooks and phases](/rhai/hooks-and-phases.md) — request vs response hook
- [Rules and actions](/policy-routing/rules-and-actions.md) — YAML equivalents for many `txn` methods
