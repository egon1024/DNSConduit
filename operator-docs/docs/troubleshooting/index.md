# Troubleshooting

Symptom-oriented pointers for common operator issues. Each section links to the canonical topic page — this hub does not duplicate full configuration reference.

| Section | Covers |
|---------|--------|
| [DNS and forwarding](#dns-and-forwarding) | Startup bind failures, no client reply, **SERVFAIL**, upstream timeouts |
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
| **No** DNS reply and **no** error RCODE (silent) | [Parse](/concepts/architecture-and-packet-path.md#parse) drop or policy **`drop`** | Malformed wire, multi-question query, or matching rule/script drop — [`conduit_parse_rejected_total`](/observability/built-in-metrics.md#conduit_parse_rejected_total) vs policy; [Architecture — Parse](/concepts/architecture-and-packet-path.md#parse), [Rules and actions](/policy-routing/rules-and-actions.md) |
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

See [`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total) (`reason` = `timeout`, `send_error`, etc.).

### Upstream timeouts and slow responses

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Answers often **`SERVFAIL`** after ~2s (default) | **`forward.timeout_ms`** exceeded | Default **2000** ms when `forward:` omitted — [Minimal configuration — Defaults](/getting-started/minimal-configuration.md#defaults-you-do-not-need-to-write-yet); increase timeout or fix upstream latency |
| Counter `reason="timeout"` on forward errors | Upstream slow or down | [`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total); test backend with `dig @BACKEND_IP -p PORT` |
| `reason="table_full"` on forward errors | Too many in-flight queries to one backend | Lower load or raise **`forward.outstanding_per_backend`** (default **100**) — [Architecture — Forward](/concepts/architecture-and-packet-path.md#forward) |
| Wrong egress path (multi-homed host) | Source bind or pool **`sources_*`** | [Dual-stack forwarding](/guides/dual-stack-forwarding.md) |

Response-hook **`retry`** can run after timeout — Conduit still reaches [Response rules](/concepts/architecture-and-packet-path.md#response-rules) so policy can fail over to another pool.

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

Authentication headers for collectors are **not** operator-supported yet. Bind Prometheus scrape to loopback or restrict with firewall — scrape has **no built-in auth** today.

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
- [Control plane workflows](/guides/control-plane-workflows.md) — reload, apply, export, and restart
- [Config file](/control-plane/config-file.md) — validation, startup path, startup vs reload
- [Configuration model](/control-plane/configuration-model.md) — overlay, last-good snapshot, pending reconcile
- [Observability](/observability/index.md) — which signal to use, OTEL naming, reload matrix
- [Metrics and tracing](/guides/metrics-and-tracing.md) — metrics + tracing lab
- [Event export and dnstap](/guides/event-export-dnstap.md) — dnstap lab
- [Operator metrics profiles](/guides/operator-metrics-profiles.md) — **`minimal`** vs **`full`** scrape comparison
