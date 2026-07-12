# Troubleshooting

Symptom-oriented pointers for common operator issues. Each section links to the canonical topic page — this hub does not duplicate full configuration reference.

| Section | Covers |
|---------|--------|
| [DNS and forwarding](#dns-and-forwarding) | Startup bind failures, no client reply, **SERVFAIL**, upstream timeouts |
| [Backend health](#backend-health) | Probes, passive fast-trip, fail-open, [drain](/glossary/index.md#drain)/[freeze](/glossary/index.md#freeze), health metrics |
| [Dataplane runtime and concurrency](#dataplane-runtime) | Slot-pool exhaustion, `split_io` runtime and worker misconfiguration |
| [Control plane](#control-plane) | **`conduitctl`** connectivity, rejected reload/apply, overlay surprises, restart-pending changes |
| [Observability](#observability) | Metrics scrape, OTEL push, dnstap, tracing, logging |

## DNS and forwarding { #dns-and-forwarding }

### Conduit exits at startup or never serves DNS

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Process exits immediately; errors on stderr | Invalid YAML, validation, or snapshot compile at **first startup** | `conduitctl validate --file PATH`; fix messages (`script '…'`, `rule '…'`, `data source '…'`, field validation) — [Config file — Validation](/control-plane/config-file.md#validation) |
| `Address already in use` / bind error in logs | Listener port taken, or **`threads` > 1** on UDP without **`reuse_port: true`** | [Reference: listeners](/reference/config-schema/listeners.md) (`reuse_port`, `threads`); `ss -ulnp \| grep PORT` |
| `Permission denied` binding port **53** (or other privileged port) | Process lacks bind capability | Run as root, use **`CAP_NET_BIND_SERVICE`**, or bind a high port (for example **15353**) — [Install and run](/getting-started/install-and-run.md) |
| Process runs but clients get no answer | Empty `listeners.listeners` or no pool/backends | At least one listener and one pool with a backend — [Config file — What makes a config runnable](/control-plane/config-file.md#what-makes-a-config-runnable) |

`conduitctl validate` does **not** bind sockets — bind failures appear at **startup** or after **restart**, not during validate alone.

Confirm listeners after a successful start:

```bash
ss -ulnp | grep conduit
# or match your listener port from config
```

### Client gets no response (timeout or silence)

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `dig` **times out** | Conduit not listening, wrong host/port, or firewall | Process running? `listeners.listeners[].address` matches `@host -p port`? — [First query](/getting-started/first-query.md) |
| **No** DNS reply and **no** error RCODE (silent) | [Parse](/concepts/architecture-and-packet-path.md#parse) drop or policy **`drop`** | Malformed wire → [`conduit_parse_rejected_total`](/observability/built-in-metrics.md#conduit_parse_rejected_total); policy → [`conduit_queries_dropped_total`](/observability/built-in-metrics.md#conduit_queries_dropped_total) (`request_rules` / `response_rules`); [Architecture — Parse](/concepts/architecture-and-packet-path.md#parse), [Rules and actions](/policy-routing/rules-and-actions.md) |
| **`REFUSED`** from `dig` | Query sent to wrong service/port | Client targets Conduit listener, not upstream backend port — [First query — If the query fails](/getting-started/first-query.md#if-the-query-fails) |
| Query worked before config change | Reload rejected — still on [last-good snapshot](/glossary/index.md#last-good-snapshot) | Logs for validation errors; `conduitctl validate --file` on the file you deployed — [Control plane — Reload or apply fails](#reload-or-apply-fails-validation) |

Quick path check (adjust ports):

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 example.com A
ss -ulnp | grep -E '15353|5300'
```

### SERVFAIL or other error RCODE

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| **`SERVFAIL`** on most queries | Upstream unreachable, wrong pool/backend, or forward failure | Backend `address` correct and reachable? Upstream resolver running? — [First query](/getting-started/first-query.md#if-the-query-fails) |
| **`SERVFAIL`** after retries exhausted | **`max_attempts`**, pool exhausted, or **`max_txn_duration_ms`** | [Retries and transactions](/policy-routing/retries-and-transactions.md); [`conduit_retries_total`](/observability/built-in-metrics.md#conduit_retries_total) |
| **`SERVFAIL`** immediately (no upstream wait) | Missing pool, empty backends, or route failure | Pool name from rules matches `pools:`; at least one backend — [Pools and backends](/policy-routing/pools-and-backends.md) |
| **`NXDOMAIN`** / other RCODE | Upstream answer or policy **`set_rcode`** | Expected upstream behavior vs response-hook policy — [Rules and actions](/policy-routing/rules-and-actions.md) |

When [metrics](/observability/metrics.md) are enabled:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_forward_errors
```

See [`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total) (`pool`, `backend`, `reason` = `timeout`, `send_error`, etc.).

### Upstream timeouts and slow responses

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Answers often **`SERVFAIL`** after ~2s (default) | **`forward.timeout_ms`** exceeded | Default **2000** ms — [Reference: forward](/reference/config-schema/forward.md); increase timeout or fix upstream latency |
| Counter `reason="timeout"` on forward errors | Upstream slow or down | [`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total); test backend with `dig @BACKEND_IP -p PORT` |
| `reason="table_full"` on forward errors | Too many in-flight queries to one backend | Lower load or raise **`forward.outstanding_per_backend`** (default **100**) — [Reference: forward](/reference/config-schema/forward.md) |
| Wrong egress path (multi-homed host) | Source bind or pool **`sources_*`** | [Dual-stack forwarding](/guides/dual-stack-forwarding.md) |

Response-hook **`retry`** can run after timeout — Conduit still reaches [Response rules](/concepts/architecture-and-packet-path.md#response-rules) so policy can fail over to another pool.

## Backend health { #backend-health }

For probes, passive fast-trip, eligibility, and operator controls, see [Backend health](/policy-routing/backend-health.md). Health is **opt-in** (`pools[].health.enabled: true`).

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Traffic still hits a dead backend | Health **disabled** for the pool (default) | Enable `health:` on the pool — [Backend health — When to enable](/policy-routing/backend-health.md#when-to-enable-health) |
| All backends marked down; queries still succeed | [Fail-open floor](/glossary/index.md#fail-open-floor) or single-backend pool | `min_eligible`; single-backend pools always fail open — [Backend health — Route](/policy-routing/backend-health.md#route-eligibility-weight-and-fail-open) |
| Backend stays down after upstream recovers | Waiting for probe **`rise`**; passive fast-trip cannot mark **up** | Wait for `rise` consecutive successful probes, or `conduitctl health set up` / **`health resume`** — [Active probes and passive fast-trip](/policy-routing/backend-health.md#active-probes-and-passive-fast-trip) |
| Drained backend returns to rotation unexpectedly | Scope not [frozen](/glossary/index.md#freeze), or **`health resume`** snapped applied to observed | `conduitctl health show`; drain is `health set down` (implies freeze) — [Operator controls](/policy-routing/backend-health.md#operator-controls-freeze-drain-resume) |
| Applied health stale after clear/freeze sequence | Clear-while-frozen footgun | Prefer atomic **`conduitctl health resume`** — [Clear-while-frozen](/policy-routing/backend-health.md#clear-while-frozen-footgun) |
| `conduitctl health` fails | No control plane at process start | Same as [conduitctl cannot connect](#conduitctl-cannot-connect) |
| Health gauges missing from scrape | Profile not **`full`**, or health not enabled | `metrics.profile: full` and at least one pool with health — [Built-in metrics — Backend health](/observability/built-in-metrics.md#backend-health) |
| High probe load on upstreams | Low `interval_ms` × many backends | Raise interval; size for upstream tolerance — [Probe behavior](/policy-routing/backend-health.md#probe-behavior-operator-view) |

```bash
conduitctl health show
curl -sS "http://127.0.0.1:9090/metrics" | grep -E 'conduit_backend_health_|conduit_probe_results'
```

Process logs: active-probe transitions at INFO (`backend health transition`); passive fast-trip at WARN (`passive health: forward failure`, `passive fast-trip: backend marked down`).

## Dataplane runtime and concurrency { #dataplane-runtime }

For runtime models, worker pools, and the [transaction](/glossary/index.md#transaction) slot pool, see [Runtime and concurrency](/concepts/runtime-and-concurrency.md) and the [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) guide. Settings under **`dataplane:`**, **`listeners:`**, and **`forward:`** are **start-time** — a [reload](/glossary/index.md#reload-from-disk) or `conduitctl apply` updates the stored snapshot but you must **restart** to apply them on the wire.

### Slot pool exhaustion

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| [`conduit_slot_pool_exhausted_total`](/observability/built-in-metrics.md#conduit_slot_pool_exhausted_total) rising; queries shed under load | In-flight transactions hit **`orchestrator.txn_table_capacity`** (default **1024**) | Raise `orchestrator.txn_table_capacity` and **restart**; reduce inbound load — [Runtime and concurrency — Transaction slot pool](/concepts/runtime-and-concurrency.md#transaction-slot-pool) |
| [`conduit_slots_in_use`](/observability/built-in-metrics.md#conduit_slots_in_use) sits near [`conduit_slots_capacity`](/observability/built-in-metrics.md#conduit_slots_capacity) | Concurrency near the ceiling — under **`split_io`**, parked upstream waits hold slots while waiting | Size capacity ≈ peak query rate × transaction duration, with headroom — [Dataplane runtime tuning — slot pool](/guides/dataplane-runtime-tuning.md#sizing-the-slot-pool-and-concurrency-caps) |
| **`SERVFAIL`** spikes on **one pool** only under load | **`pools[].max_inflight`** concurrent-forward cap reached (`split_io`) | Raise or remove the pool `max_inflight` and **restart** — [Reference: pools — Per-pool in-flight limit](/reference/config-schema/pools.md#per-pool-in-flight-limit) |

Slot gauges require the **`full`** [metrics profile](/observability/built-in-metrics.md#profiles); the exhaustion counter is exported on both profiles:

```bash
curl -sS "http://127.0.0.1:9090/metrics" \
  | grep -E '^conduit_(slots_in_use|slots_capacity|slot_pool_exhausted_total)'
```

### Split I/O runtime or worker misconfiguration

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| **`policy_workers`** / **`io_workers`** appear to have no effect | Runtime is **`sync`** (or `dataplane:` omitted) — those pools exist only under `split_io` | Set **`dataplane.runtime: split_io`**; under `sync` the ingress thread does everything — [Reference: dataplane — Worker pools](/reference/config-schema/dataplane.md#worker-pools-split_io) |
| `Address already in use` with **`threads` > 1** on UDP | Missing **`reuse_port`** | Set **`listeners.reuse_port: true`** (Unix, UDP) — [Reference: listeners — `reuse_port` and `threads`](/reference/config-schema/listeners.md#reuse_port-and-threads) |
| Changed **`dataplane.runtime`** or worker counts — no change on the wire | Start-time settings; reload/apply only update the snapshot | **Restart** `conduit` — [Reference: dataplane — Reload and restart](/reference/config-schema/dataplane.md#reload-and-restart) |
| High [`lookup`](/observability/built-in-metrics.md#conduit_phase_duration_seconds) phase time, but ingress keeps accepting queries | **Expected** under `split_io` when upstream is slow — Lookup includes parked forward waits without stalling ingress | This is an upstream/forward issue, not a worker shortage — see [Upstream timeouts](#upstream-timeouts-and-slow-responses); [`conduit_forward_outstanding`](/observability/built-in-metrics.md#conduit_forward_outstanding) shows concurrent waits |
| Ingress stalls during slow upstreams under **`sync`** | `sync` blocks the ingress thread on the upstream wait | Switch to `split_io`, or add ingress `threads` — [Runtime and concurrency — Sync runtime](/concepts/runtime-and-concurrency.md#sync-runtime-default), [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md#when-to-use-sync-vs-split_io) |

## Control plane { #control-plane }

### conduitctl cannot connect

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Connection refused to `127.0.0.1:5199` | No **`control:`** at **process start** | Add `control.listen_address` and **restart** — [gRPC and conduitctl — Enabling the control plane](/control-plane/grpc-and-conduitctl.md#enabling-the-control-plane) |
| Worked before; fails after config edit | `control:` added via reload only — listener not started | **Restart** after enabling control — snapshot updates but gRPC binds at startup |
| TLS / protocol errors | **`http://`** vs **`https://`** mismatch | Plain TCP → `http://`; with **`control.tls`** → `https://` — [gRPC and conduitctl — Connecting](/control-plane/grpc-and-conduitctl.md#connecting) |
| **`Unauthenticated`** / RPC denied | **`control.api_keys`** set; missing or wrong key | `--api-key` / `CONDUIT_API_KEY` — [API keys](/security/api-keys.md) |
| mTLS required | **`control.tls.client_ca_path`** set | Client cert + API key rules — [mTLS](/security/mtls.md) |

`conduitctl validate --file` runs **offline** and does not need the control plane.

### Reload or apply fails validation { #reload-or-apply-fails-validation }

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `conduitctl reload` exits non-zero; DNS still works | Bad on-disk YAML; reload rejected | **[Last-good snapshot](/glossary/index.md#last-good-snapshot)** still active — fix file, validate, reload again — [Config file — Startup vs reload](/control-plane/config-file.md#startup-vs-reload) |
| **SIGHUP** sent; no config change | Same as reload failure | Process logs for validation/compile errors |
| `conduitctl apply` exits non-zero | Patch fails validate/compile | Sparse patch only; no forbidden keys — [Configuration model — overlay](/control-plane/configuration-model.md#overlay) |
| Startup exits; DNS never worked | First snapshot never installed | Fix stderr from `conduit` or `validate --file` before retrying start |

After a failed reload, **`conduitctl export`** still reflects the **running** effective config (last good), not the rejected file.

### Apply rejected or overlay surprises

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Apply error mentions **`rules`**, **`metrics`**, or **`tracing`** | Those sections are **file-layer only** | Edit startup YAML and **reload** — not overlay-eligible — [Configuration model — overlay](/control-plane/configuration-model.md#overlay) |
| Weights reverted unexpectedly | **Reload** or **SIGHUP** clears overlay | Expected — reload re-reads disk and drops in-memory patches — [Control plane workflows](/guides/control-plane-workflows.md) |
| **`export`** differs from file on disk | Active **overlay** or normalized defaults | Compare `export` to file; use **`apply --clear`** to drop overlay without re-reading disk — [Reload and export — clear vs reload](/control-plane/reload-and-export.md#clear-vs-reload) |
| Patched wrong startup file | Reload always uses path from process start | Edit the file Conduit was **started with**, not only a copy used with `validate --file` — [Config file — Overview](/control-plane/config-file.md#overview) |

### Listener, forward, or control change had no effect

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Log **`listeners: pending (restart required)`** | Bind address, `threads`, or `reuse_port` changed | Reload updated snapshot; **restart** `conduit` to rebind — [Workflow 5 — Hot reload vs process restart](/guides/control-plane-workflows.md#workflow-5-hot-reload-vs-process-restart) |
| Log **`forward egress: pending (restart required)`** | Egress sockets or transport changed | **Restart** after reload — [Configuration model — Pending reconcile](/control-plane/configuration-model.md#pending-reconcile-restart-required) |
| New **`metrics:`** / **`tracing:`** / **`logging:`** ignored after reload | Export and subscriber bind at **process start** | **Restart** — [Observability — Changing observability config](/observability/index.md#changing-observability-config) |
| New dnstap sink or destination after reload | Event consumer threads start at process start | **Restart** — [Event export — Changing events config](/observability/event-export.md#changing-events-config) |

## Observability { #observability }

### Metrics scrape returns connection refused or empty

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `curl` to `/metrics` fails with connection refused | `metrics.prometheus` not configured, metrics disabled, or Conduit not listening on that address | Confirm `metrics.enabled: true` and `metrics.prometheus.listen_address` in the **startup** config; [Metrics](/observability/metrics.md) |
| Connection works but no `conduit_*` series | `metrics:` omitted or `enabled: false` | Built-ins are off when the block is missing — [Metrics — Enabling export](/observability/metrics.md#enabling-export) |
| Scrape works but counters stay at zero | No DNS traffic yet, or wrong listener port in `dig` | Send a query to the configured listener; check [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) |
| Changed scrape address or enabled metrics after start — still old behavior | Export listeners bind at **process start** | **Restart** Conduit after `metrics:` changes — [Observability — Changing observability config](/observability/index.md#changing-observability-config) |

Smoke test:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | head
dig @127.0.0.1 -p 15353 +time=3 example.com A
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_queries
```

Adjust host, scrape port, and listener port to your config.

### OTEL metrics push failures

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Log line `failed to build OTLP metric exporter` at startup | Invalid `metrics.otel.endpoint` (not `http://` or `https://`) | [Metrics — Export architecture](/observability/metrics.md#export-architecture); validate with `conduitctl validate --file` |
| Periodic `otel metrics push failed` at **`warn`** | Collector down, TLS verify failure, or network block | Endpoint reachable; for self-signed HTTPS use `allow_invalid_certs: true` (lab only) or fix collector cert |
| No push logs | Push interval default **15s**; successes log at **`debug`** only | Set `logging.level: debug` briefly to see `otel metrics push ok` |
| Enabled OTEL after process start — no push | OTEL task starts at **process start** | **Restart** after adding or changing `metrics.otel` |
| Push rejected with **401** / **403** | Collector requires auth | Set **`metrics.otel.headers`** (for example `Authorization: Bearer …`) — [Metrics — OTEL](/observability/metrics.md#export-architecture) |

Bind Prometheus scrape to loopback or restrict with firewall — scrape has **no built-in auth** today.

### Event export / dnstap gaps

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| No frames at collector | Collector not running, wrong socket path, or sink filters exclude the query | Start **`conduit-dnstap-tracer`** before Conduit; see [Event export and dnstap](/guides/event-export-dnstap.md) |
| `conduit_events_queue_dropped_total` increasing | Collector slow or down; queue full | [Event export — Overload and metrics](/observability/event-export.md#overload-and-metrics); fix collector throughput |
| Added a new sink via reload — no effect | New sinks require **restart** | [Event export — Changing events config](/observability/event-export.md#changing-events-config) |
| Query frames missing pool/backend on `query` emit | Expected — pool/backend filters apply to **`response`** / **`retry`** only | [Event export — Filters](/observability/event-export.md#filters) |

### Pipeline trace not found

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `conduitctl trace N` — not found | Wrong `txn_id`, trace expired, or activation did not match | [Tracing — Activation](/observability/tracing.md#activation); traces TTL **5 minutes**, store cap **1000** |
| Control plane unavailable | No `control:` at startup | `conduitctl trace` needs gRPC — [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md) |
| Unsure of `txn_id` | Id is per-worker and increments | Set `logging.level: debug`, send query, read `txn_id` from **`query complete`** — [Logging](/observability/logging.md#per-query-summaries) |
| Enabled tracing after start — no traces | Tracing compiled at **process start** | **Restart** after `tracing:` changes — [Tracing — Changing tracing config](/observability/tracing.md#changing-tracing-config) |

### Logging surprises

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| No per-query lines at default level | **`query complete`** is **`debug`** only | By design — use [Metrics](/observability/metrics.md) for volume; enable **`debug`** briefly for `txn_id` |
| `RUST_LOG` overrides config | Env set at startup | Unset `RUST_LOG` in production — [Logging — RUST_LOG override](/observability/logging.md#rust_log-override) |
| Changed `logging.level` via reload — no effect | Subscriber binds at **process start** | **Restart** after logging changes — [Logging — Changing logging config](/observability/logging.md#changing-logging-config) |

## Related topics

- [Getting started — First query](/getting-started/first-query.md) — end-to-end lab and basic `dig` failures
- [Guide: Backend health](/guides/backend-health.md) — probes, drain, and resume lab
- [Control plane workflows](/guides/control-plane-workflows.md) — reload, apply, export, and restart
- [Config file](/control-plane/config-file.md) — validation, startup path, startup vs reload
- [Configuration model](/control-plane/configuration-model.md) — overlay, last-good snapshot, pending reconcile
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — runtime models, worker pools, slot pool
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) — `sync` vs `split_io`, worker sizing, slot pool
- [Observability](/observability/index.md) — which signal to use, OTEL naming, reload matrix
- [Metrics and tracing](/guides/metrics-and-tracing.md) — metrics + tracing lab
- [Event export and dnstap](/guides/event-export-dnstap.md) — dnstap lab
- [Operator metrics profiles](/guides/operator-metrics-profiles.md) — **`minimal`** vs **`full`** scrape comparison
