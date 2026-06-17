# Hooks and phases

[Rule Rhai](/rhai/index.md) runs at two fixed points in the query [pipeline](/concepts/architecture-and-packet-path.md#pipeline-phases): the **request hook** and the **response hook**. This page explains those hooks from a **script author’s** perspective — how they differ, which `txn` APIs each hook allows, and how request and response scripts work on the same [transaction](/glossary/index.md#transaction).

For hook timing, pipeline placement, first-match evaluation, built-in actions, and YAML wiring, see [Rules and actions](/policy-routing/rules-and-actions.md). For method-level detail (`txn.set_pool`, `txn.drop_query`, …), see [Transaction API](/rhai/transaction-api.md).

## Request vs response (script perspective)

| | Request hook | Response hook |
|---|--------------|---------------|
| **When your script runs** | Before [Route](/concepts/architecture-and-packet-path.md#route) and [Forward](/concepts/architecture-and-packet-path.md#forward) | After [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response) (answer or timeout) |
| **Runs again on retry?** | No — once per transaction | Yes — once per forward attempt |
| **Upstream answer / rcode** | Not available yet | Available (`txn.response()`, `txn.response_rcode()`) |
| **Retry / failover** | Not available (`txn.request_retry()` has no effect) | Available (`txn.set_retry_pool`, `txn.request_retry()`) |
| **Egress override** | Yes (`txn.set_source_v4` / `set_source_v6`) | Phase error — source was chosen before Forward |
| **Typical use** | Classify, route, tag, block before upstream | Act on rcode, timeout, latency; retry or metric |

On **retry**, the request hook does **not** re-run. Tags and pool choice from the first request pass remain on the transaction unless response policy changes them. See [Retries and transactions](/policy-routing/retries-and-transactions.md).

## Phase guards

Some `txn` methods and helpers are restricted to one hook. Calling them on the wrong hook does **not** drop the query: Conduit logs a warning, skips further script effects for that hook invocation, and continues the pipeline ([Sandbox limits](/rhai/sandbox-limits.md) — fail-open).

| API | Request hook | Response hook |
|-----|--------------|---------------|
| `txn.question()`, `question_qname(txn)` | Yes | Yes (same question) |
| `txn.response()`, `txn.response_rcode()` | Error / empty | Yes |
| `txn.set_pool`, `txn.set_tag`, `txn.has_tag` | Yes | Yes |
| `txn.set_source_v4`, `txn.set_source_v6` | Yes | Phase error |
| `txn.set_retry_pool`, `txn.request_retry()` | No effect | Yes |
| `txn.drop_query()`, `txn.set_rcode()` | Yes | Yes |
| `txn.sample_percent`, `table_lookup` | Yes | Yes |
| `txn.elapsed_ms()`, `txn.get_attempt_count()` | Yes | Yes |
| `txn.metric_inc` / `metric_inc_labels` | Yes | Yes |

Full reference: [Transaction API](/rhai/transaction-api.md).

## Pairing request and response scripts

Request and response hooks operate on the same [transaction](/glossary/index.md#transaction). [Tags](/glossary/index.md#tags) and pool choice set on the request hook are still present when the response hook runs — including after a [retry](/glossary/index.md#retry) — unless response policy changes them.

A common pattern:

1. **Request** — classify the query (lookup table, qname pattern) and **`txn.set_tag`**
2. **Response** — act on upstream outcome **and** request tags (retry, metric, dnstap gate)

Example — tag suspicious logins on the request hook, increment a user metric on slow responses:

**Request rule** (`tag-suspicious.rhai`):

```rhai
if question_qname(txn) == "login.suspicious.example." {
    txn.set_tag("suspicious", true);
}
```

**Response rule** (`slow-login-alert.rhai`):

```rhai
if txn.has_tag("suspicious") {
    if txn.elapsed_ms() > 500 {
        txn.metric_inc("slow_login", 1);
    }
}
```

Runnable config: `tests/fixtures/config/with-rhai-slow-login.yaml` in the repository. Similar split patterns appear in servfail-retry (request pool + response retry) and dnstap tagging (request tag + sink `tag_required`).

## Script errors on a hook

When a script exceeds [sandbox limits](/rhai/sandbox-limits.md), throws, or hits a phase guard, Conduit logs **`rhai script error`** at **warn** severity and stops applying further script effects for that hook invocation. The query is **not** dropped solely because the script failed.

Built-in actions on the same rule have **already run** by then — see [Scripted policy (Rule Rhai)](/policy-routing/rules-and-actions.md#scripted-policy-rule-rhai). Put safety-critical effects in built-ins when you can; reserve Rhai for logic that needs it. Run **`conduitctl validate --file`** before reload so compile errors surface early.

## Related topics

- [Rhai overview](/rhai/index.md) — when to use scripts, minimal example, reload behavior
- [Transaction API](/rhai/transaction-api.md) — `txn` methods and YAML equivalents
- [Rules and actions](/policy-routing/rules-and-actions.md) — hooks, selectors, built-in actions, `rhai` wiring
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — full pipeline and phases
- [Retries and transactions](/policy-routing/retries-and-transactions.md) — `retry_pool`, attempt limits, pool exhaustion
- [Event export](/observability/event-export.md) — request tags + sink filters
