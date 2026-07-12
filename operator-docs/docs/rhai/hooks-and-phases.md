# Hooks and phases

**Rhai for rules** ([Rule Rhai](/rhai/rule-rhai.md)) runs at two defined points in the query [pipeline](/concepts/architecture-and-packet-path.md#pipeline-phases): the [request hook](#request-hook) and the [response hook](#response-hook). This page explains those hooks from a **script author’s** perspective — how they differ, which host APIs each hook allows, and how request and response scripts work on the same [transaction](/glossary/index.md#transaction).

For hook timing, pipeline placement, first-match evaluation, built-in actions, and YAML wiring, see [Rules and actions — Request and response hooks](/policy-routing/rules-and-actions.md#request-and-response-hooks).

**Host API map:** [Host API overview](/rhai/host-api.md). **`txn`** method reference: [Transaction API (`txn`)](/rhai/txn-api.md). **`runtime.routing()`**: [Runtime API](/rhai/runtime-api.md).

## Request vs response (script perspective) { #request-vs-response-script-perspective }

| | [Request hook](#request-hook) | [Response hook](#response-hook) |
|---|---|---|
| **When your script runs** | Before [Lookup](/concepts/architecture-and-packet-path.md#lookup) (cache and forward providers) | After Lookup produced a wire answer (cache hit or forward), before [Send](/concepts/architecture-and-packet-path.md#send) — or after forward timeout |
| **Runs again on retry?** | No — once per transaction | Yes — once per Lookup attempt that reaches Response rules |
| **Upstream answer / rcode** | Not available yet | Available (`txn.response()`, `txn.response_rcode()`, `txn.answer_source()`) |
| **Retry / failover** | `txn.set_retry_pool` sets pool for retry Lookup if retry occurs; first forward ignores; `txn.request_retry()` / `request_retry_now()` have no effect | `txn.set_retry_pool` sets pool for retry Lookup if retry occurs; first forward ignores; `txn.request_retry()` soft-retry; `txn.request_retry_now()` hard-retry |
| **Egress override** | Yes (`txn.set_source_v4` / `set_source_v6`) | Standing source: phase error. Retry source: yes (`txn.set_retry_source_v4` / `v6`, `clear_retry_source_*`) |
| **`runtime.routing()`** | Yes — snapshot at hook phase start | Yes — new snapshot each attempt |
| **Typical use** | Classify, route, tag, block before upstream | Act on rcode, timeout, latency; retry or metric |

### Request hook { #request-hook }

Runs once per [transaction](/glossary/index.md#transaction) at [Request rules](/concepts/architecture-and-packet-path.md#request-rules) — after [Parse](/concepts/architecture-and-packet-path.md#parse), before [Lookup](/concepts/architecture-and-packet-path.md#lookup). Your script sees the client question only; upstream answers are not available yet. On [retry](/glossary/index.md#retry), the request hook does **not** run again — tags and pool choice from the first pass stay on the transaction unless response policy changes them. In config, use `hook: request` on the rule. See [Rules and actions — Request and response hooks](/policy-routing/rules-and-actions.md#request-and-response-hooks).

### Response hook { #response-hook }

Runs at [Response rules](/concepts/architecture-and-packet-path.md#response-rules) — after [Lookup](/concepts/architecture-and-packet-path.md#lookup) stored an answer or a forward timeout occurred, before [Send](/concepts/architecture-and-packet-path.md#send) or another [Lookup](/concepts/architecture-and-packet-path.md#lookup) when policy retries. Runs **once per Lookup attempt** that reaches Response rules (skipped on cache hit when **`on_hit.response_rules: skip`**). Upstream answers and rcodes are available (`txn.response()`, `txn.response_rcode()`, `txn.answer_source()`). Tags and pool choice from the request hook remain unless this hook changes them. In config, use `hook: response` on the rule. See [Rules and actions — Request and response hooks](/policy-routing/rules-and-actions.md#request-and-response-hooks) and [Retries and transactions](/policy-routing/retries-and-transactions.md).

## Phase guards

Some **`txn`** methods are restricted to one hook. Calling them on the wrong hook does **not** drop the query: Conduit logs a warning, skips further script effects for that hook invocation, and continues the pipeline ([Sandbox limits](/rhai/sandbox-limits.md) — fail-open).

Shared scopes (**`lookup`**, **`metrics`**, **`log`**, **`runtime`**) are available on both hooks unless noted on their reference pages.

### `txn` methods

| API | Request hook | Response hook |
|-----|--------------|---------------|
| `txn.question()` | Yes | Yes (same question) |
| `txn.response()`, `txn.response_rcode()` | Error / empty | Yes |
| `txn.set_cache_lookup_eligible(bool)` | Yes | Ignored (request hook only) |
| `txn.answer_source()`, `txn.cache_instance()` | Empty on request hook | Yes when answer exists |
| `txn.clear_tag`, `txn.has_tag`, `txn.set_pool`, `txn.clear_pool`, `txn.set_tag` | Yes | Yes |
| `txn.set_source_v4`, `txn.set_source_v6` | Yes | Phase error |
| `txn.set_retry_source_v4`, `txn.set_retry_source_v6` | Stash for retry forward if retry occurs; first forward ignores | Stash for retry forward if retry occurs; first forward ignores |
| `txn.clear_retry_source_v4`, `txn.clear_retry_source_v6` | Clears `retry_source_override_*` | Clears `retry_source_override_*` |
| `txn.set_retry_pool` | Pool for retry Route if retry occurs; first Route ignores | Pool for retry Route if retry occurs; first Route ignores |
| `txn.request_retry()` | No effect | Soft retry |
| `txn.request_retry_now()` | No effect | Hard retry (stop rule); does not clear soft drop |
| `txn.clear_retry()` | No effect | Clear soft retry |
| `txn.clear_retry_pool()` | Clears `retry_pool` | Clears `retry_pool` |
| `txn.drop_query()` | Soft drop — later actions on the rule still run | Soft drop |
| `txn.drop_query_now()` | Hard drop — stops the rule immediately | Hard drop (stop rule) |
| `txn.clear_drop()` | Clears soft drop | Clears soft drop |
| `txn.set_rcode()` | Yes | Yes |
| `txn.sample_percent`, `txn.sample_percent_for_qname`, `txn.sample_percent_for_rule`, `txn.every_nth_worker`, `txn.every_nth_global` | Yes | Yes |
| `txn.elapsed_ms()`, `txn.get_attempt_count()`, `txn.last_forward_ms()`, `txn.now_unix()`, `txn.utc_hour()`, `txn.utc_weekday()` | Yes | Yes |
| `txn.txn_id()`, `txn.config_generation()`, `txn.rule_name()` | Yes | Yes |
| `txn.client_addr()`, `txn.client_ip()`, `txn.client_port()`, `txn.client_protocol()`, `txn.listener()` | Yes | Yes |
| `txn.selected_pool()`, `txn.selected_backend()`, `txn.selected_backend_name()` | Yes (may be empty pre-Route) | Yes |
| `txn.response_truncated()`, `txn.response_answer_count()`, … | Empty/false/`-1` | Yes when wire meta parsed |

Soft vs hard drop: [Transaction API — Outcomes](/rhai/txn-api.md#outcomes) (`drop_query` vs `drop_query_now`).

### Shared host scopes

| Scope | Request hook | Response hook |
|-------|--------------|---------------|
| `lookup(table, key)` | Yes | Yes |
| `metrics.inc` / `metrics.inc_labels` | Yes | Yes |
| `log.info` / `log.warn` | Yes | Yes |
| `runtime.routing()` | Yes | Yes |

Full **`txn`** reference: [Transaction API (`txn`)](/rhai/txn-api.md).

**`txn.request_retry_now()`** does not clear soft drop. If `txn.drop_query()` ran earlier on the same rule, the outcome is still **drop** unless you call **`txn.clear_drop()`** first. See [Outcome at end of rule](/policy-routing/rules-and-actions.md#outcome-at-end-of-rule).

## Pairing request and response scripts { #pairing-request-and-response-scripts }

The [request hook](#request-hook) and [response hook](#response-hook) operate on the same [transaction](/glossary/index.md#transaction). [Tags](/glossary/index.md#tags) and pool choice set on the request hook are still present when the response hook runs — including after a [retry](/glossary/index.md#retry) — unless response policy changes them.

A common pattern:

1. **[Request hook](#request-hook)** — classify the query (`lookup`, qname pattern) and **`txn.set_tag`**
2. **[Response hook](#response-hook)** — act on upstream outcome **and** request tags (retry, metric, dnstap gate)

Example — tag suspicious logins on the request hook, increment a user metric when the **upstream forward** was slow:

**Request rule** (`tag-suspicious.rhai`):

```rhai
if txn.question().qname == "login.suspicious.example." {
    txn.set_tag("suspicious", true);
}
```

**Response rule** (`slow-login-alert.rhai`):

```rhai
if txn.has_tag("suspicious") {
    if txn.last_forward_ms() > 500 {
        metrics.inc("slow_login", 1);
    }
}
```

Runnable config: `tests/fixtures/config/with-rhai-slow-login.yaml` in the repository. For **tag + response-side logic**, see the slow-login pattern above. For **tag-gated dnstap export**, pair request **`set_tag`** with sink **`tag_required`** — [Event export — Filters](/observability/event-export.md#filters) and [Event export and dnstap — Optional checks](/guides/event-export-dnstap.md#5-optional-checks). **SERVFAIL** failover to another pool is usually declarative (**`set_retry_pool`** + **`retry`**) — [Retries and transactions — Declarative examples](/policy-routing/retries-and-transactions.md#declarative-examples).

Health-aware failover in Rhai uses **`runtime.routing()`** on the request hook — see [Runtime API](/rhai/runtime-api.md) and fixture `routing-pool-failover.rhai`.

## Script errors on a hook

When a script exceeds [sandbox limits](/rhai/sandbox-limits.md), throws, or hits a phase guard, Conduit logs **`rhai script error`** at **warn** severity and stops applying further script effects for that hook invocation. The query is **not** dropped solely because the script failed.

Built-in actions on the same rule have **already run** by then — see [Scripted policy (Rhai for rules)](/policy-routing/rules-and-actions.md#scripted-policy-rule-rhai). Put safety-critical effects in built-ins when you can; reserve Rhai for logic that needs it. Run **`conduitctl validate --file`** before reload so compile errors surface early.

## Related topics

- [Rhai overview](/rhai/index.md) — Rhai for rules and host API map
- [Host API overview](/rhai/host-api.md) — five scope objects
- [Rhai for rules](/rhai/rule-rhai.md) — when to use scripts, minimal example, reload behavior
- [Transaction API (`txn`)](/rhai/txn-api.md) — per-query policy methods
- [Runtime API](/rhai/runtime-api.md) — `runtime.routing()` health reads
- [Rules and actions](/policy-routing/rules-and-actions.md) — hooks, selectors, built-in actions, `rhai` wiring
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — full pipeline and phases
- [Retries and transactions](/policy-routing/retries-and-transactions.md) — pool/source lifecycle, `set_retry_pool`, `retry`, attempt limits
- [Event export](/observability/event-export.md) — request tags + sink filters
