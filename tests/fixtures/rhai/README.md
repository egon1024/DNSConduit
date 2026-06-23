# Rhai fixture examples

Runnable Rhai policy scripts for development and CI. Each script under this directory has a matching config under `tests/fixtures/config/with-rhai-*.yaml`. Operator-facing walkthroughs live in **`operator-docs/`** ([Rhai policy guide](https://github.com/egon1024/DNSConduit/blob/main/operator-docs/docs/guides/rhai-policy.md)).

## Manual try workflow

Conduit **forwards** queries to the `pools` backends in the YAML. If nothing is listening on those ports, logs show `forward recv timeout` and `dig` gets **SERVFAIL** after `forward.timeout_ms` (not a successful answer).

### Mock upstreams (for a real A record)

Example 1 uses `127.0.0.1:5300` (default) and `127.0.0.1:5301` (vip). With [dnsmasq](https://dnsmasq.org/):

```bash
# Terminal A
dnsmasq --port=5300 --bind-interfaces --listen-address=127.0.0.1 \
  --no-resolv --address=/foo.vip.example/192.0.2.1

# Terminal B (vip pool — different IP proves routing)
dnsmasq --port=5301 --bind-interfaces --listen-address=127.0.0.1 \
  --no-resolv --address=/foo.vip.example/192.0.2.99
```

### Run Conduit and query

```bash
cargo run -p conduit -- tests/fixtures/config/with-rhai-vip-pool.yaml
dig @127.0.0.1 -p 15353 foo.vip.example A
```

With both dnsmasq instances, expect **192.0.2.99** and logs showing backend `127.0.0.1:5301` / pool `vip`.

Optional: `cargo run -p conduit-dnstap-tracer -- -u /tmp/dnstap.sock` for dnstap examples.

Paths in configs are relative to `tests/fixtures/config/`.

**Port note:** Fixture configs use **15353**, not 5353. On many Linux desktops, UDP 5353 is used by mDNS (Chrome, Synergy, etc.), which causes `Address already in use (os error 98)`.

## Dnstap via tags

Declarative sink filters stay in YAML (`filters.tag_required`, `selectors`, `sample_percent`). Rhai sets tags; sinks gate export on those tags. Request-hook **drops** still emit **`query`** dnstap frames when sink filters pass (no **`response`** frame).

Example 4: script sets `dnstap` tag; sink uses `tag_required: dnstap`.

Example 5: `sample_percent(percent)` uses the same deterministic hash as observation `sample_percent`; tag `sampled` gates export.

## Examples

| # | Script | Config | What it shows |
|---|--------|--------|----------------|
| 1 | `set-vip-pool.rhai` | `with-rhai-vip-pool.yaml` (or `with-rhai-minimal.yaml`) | VIP pool routing |
| 2 | `blocklist.rhai` | `with-rhai-blocklist.yaml` | CSV `table_lookup` + `metric_inc` + soft drop |
| 2b | `blocklist.rhai` | `with-rhai-blocklist-dnstap.yaml` | Same script; **`query`** dnstap on policy drop |
| 3 | `servfail-retry.rhai` | `with-rhai-servfail-retry.yaml` | Rhai API parity for **`set_retry_pool`** + **`retry`** — prefer declarative YAML in production |
| 4 | `mark-for-dnstap.rhai` | `with-rhai-dnstap-tag.yaml` | Tag-gated dnstap |
| 5 | `smart-sample.rhai` | `with-rhai-sample.yaml` | Script sampling + `tag_required: sampled` |
| 6 | `tag-suspicious.rhai` + `slow-login-alert.rhai` | `with-rhai-slow-login.yaml` | Request tag + response metric on slow path |
| 7 | `block-hits.rhai` | `with-rhai-block-hits.yaml` | User metrics with bounded `category` label |
| 8 | `bad-phase.rhai` | `with-rhai-bad-phase.yaml` | Phase guard: `response()` in request hook (fail-open) |
| 9 | `lookup-demo.rhai` | `with-rhai-lookup-demo.yaml` | Only configured `data_sources` visible to `table_lookup` |

**Manual lab** for ordered actions (`drop`, `drop_now`, `clear_drop`, `set_retry_pool`, rhai interleaving): [`tests/manual/ordered-rule-actions.md`](../../manual/ordered-rule-actions.md) and [`tests/manual/config/09-ordered-actions.yaml`](../../manual/config/09-ordered-actions.yaml).

## Action list order

On a matching rule, **every** `actions:` entry runs top to bottom — built-ins and `type: rhai` are interleaved at the position written. Put critical built-ins **above** a `rhai` step when they must run before script logic or when the script might fail.

Example 3 (`with-rhai-servfail-retry.yaml`): exercises Rhai **`set_retry_pool`** + **`request_retry()`** on SERVFAIL. Equivalent declarative policy uses **`set_retry_pool`** + **`retry`** on the response rule — see operator-docs [Retries and transactions](https://github.com/egon1024/DNSConduit/blob/main/operator-docs/docs/policy-routing/retries-and-transactions.md).

## Rhai API notes

Some Rhai reserved words require different names in scripts:

| Design / doc name | Script name | Notes |
|-------------------|-------------|-------|
| `retry(pool)` | `set_retry_pool(pool)` + `request_retry()` | Pool for retry Route if retry occurs; first Route ignores — pair with `request_retry()` / `request_retry_now()` on response |
| `retry()` (same pool) | `request_retry()` | Response hook only |
| `drop()` (soft) | `drop_query()` | Later actions on the rule still run |
| `drop_now()` (hard) | `drop_query_now()` | Stops further actions on the rule |
| `clear_drop()` | Clears soft-drop intent on the rule |
| `clear_retry()` | Clears soft-retry intent on the rule (response hook) |
| `clear_retry_pool()` | Clears `retry_pool` on the transaction |
| `lookup(table, key)` | `table_lookup(table, key)` | |
| `question().qname` | `question_qname(txn)` global | |
| `attempt_count()` in `if` | `get_attempt_count()` method | |

## Lookup grant model

Only tables listed under top-level `data_sources` in config are visible to `table_lookup(table, key)`. Scripts cannot read arbitrary files. Example 9 (`lookup-demo.rhai`) tags `region` from the `geo` CSV.
