# Release notes — 1.0.0

DNS Conduit is a **DNS forwarding and observability platform**. It sits in the query path between clients and upstream resolvers: clients send queries to Conduit [listeners](/glossary/index.md#listener), Conduit applies policy, selects an upstream [pool](/glossary/index.md#pool) and [backend](/glossary/index.md#backend), forwards the query, and returns the answer (or a policy-driven outcome). It is a **forwarder** — not an authoritative nameserver and not a standalone recursive resolver. See [What Conduit is for](/getting-started/what-conduit-is-for.md).

## DNS ingress and forwarding

- **UDP and TCP listeners** with multiple listener entries, per-listener worker threads, and `SO_REUSEPORT` for UDP fan-out. See [Reference: listeners](/reference/config-schema/listeners.md).
- **Upstream forwarding** to configured `pools:` with a default upstream timeout, per-backend concurrency caps, and EDNS/`OPT` passthrough. See [Reference: forward](/reference/config-schema/forward.md).
- **Dual-stack egress** — global, per-pool, and per-query IPv4/IPv6 source-address selection for multi-homed hosts. See [Dual-stack forwarding](/guides/dual-stack-forwarding.md).

## Policy and routing

- **Declarative rules** on the request and response hooks. Selectors match on qname, qtype, client subnet, tags, and more. See [Rules and actions](/policy-routing/rules-and-actions.md).
- **Actions** include set pool, set tags, set egress source (`set_source_v4` / `set_source_v6`), retry egress (`set_retry_source`), silent **drop**, response-hook **retry** / **retry_now**, and per-rule `sample_key` for trace/event sampling. See [Rules and actions](/policy-routing/rules-and-actions.md).
- **Pools and backends** — group upstream resolvers, load-balance with sticky weighted selection, and define a default pool. When routing or forwarding cannot produce an answer, Conduit synthesizes **SERVFAIL** by default; response-hook policy can override that outcome — set a different response code (`set_rcode`) or **drop** the query with no reply. See [Pools and backends](/policy-routing/pools-and-backends.md) and [Retries and transactions](/policy-routing/retries-and-transactions.md).
- **Retries and transactions** — response-hook retries with per-attempt backend exclusion and global caps (`txn_table_capacity`, attempt and duration limits). See [Retries and transactions](/policy-routing/retries-and-transactions.md).

## Rhai scripting

- **Embedded [Rhai](/rhai/index.md) for rule policy** on the request and response hooks, attached to matching rules as one action among the declarative ones. Scripts drive routing, tagging, drop/retry, egress, metrics, and logging; **modifying DNS packet contents and record data** (for example qname or answer rewriting) from Rhai is **not yet supported**, with work to support these features planned for future releases.
- **Five host scopes** — `txn` (per-query policy), `runtime` (read-only process state, including `runtime.routing()` for health-aware branching), `lookup` (read-only CSV tables from `data_sources:`), `metrics` (write-only `conduit_user_*` counters), and `log` (script log lines). See [Host API overview](/rhai/host-api.md).
- **Compile-time validation** — Rhai syntax errors fail reload rather than deferring to query time.
- **Sandbox limits** bound script execution. See [Sandbox limits](/rhai/sandbox-limits.md).

## Lookup spine and answer cache

- **Lookup pipeline phase** with an ordered provider chain. Configs that omit `lookup:` use an implicit forward-only profile with no cache allocation on the hot path. See [Reference: lookup](/reference/config-schema/lookup.md).
- **Optional in-memory DNS answer cache** — add a cache provider before forward to serve stored wire answers from memory. See [DNS answer cache](/guides/dns-answer-cache.md) and [Reference: caches](/reference/config-schema/caches.md). Behavior includes:
    - **Negative caching** (NXDOMAIN/NODATA, optional descendant coverage, and SERVFAIL TTL).
    - **Single-flight** for parallel identical misses — one upstream fetch, waiters resume on fill.
    - **TTL decay on serve**, per-query Question/EDNS echo (including 0x20 QNAME encoding), and answers shared across UDP and TCP.
    - Optional **`truncated_udp`** TC=1 stubs, **`rotate_rrset_on_serve`**, **`on_hit.response_rules`** control, and passive or active eviction.
    - Live `max_entries` updates on reload/apply; other cache policy requires a restart. Entries are in-memory only and lost on restart.

## Backend health and failover

- **Optional backend health checks** — enabled per pool, but liveness is tracked for each individual **backend** through active probes and an optional passive fast-trip (on by default; can be turned off to follow probes only). Backends found unhealthy are excluded from selection, with a **fail-open** floor so a pool keeps serving when too few (or no) backends remain up.
- **Operator controls** — freeze, drain, and resume individual backends via `conduitctl health`. See [Backend health](/policy-routing/backend-health.md) and [Reference: health](/reference/config-schema/health.md).

## Runtime and concurrency

- **Two dataplane runtime models** chosen at startup: `sync` (one thread runs the whole pipeline, including the upstream wait) and `split_io` (separate ingress, policy, and I/O worker pools so ingress keeps accepting during slow upstreams).
- **Bounded concurrency** via ingress threads, worker-pool sizes, a preallocated transaction slot pool, and per-backend outstanding caps.
- **Graceful drain on shutdown** — in-flight transactions finish (bounded by `shutdown.drain_timeout_ms`) before listener teardown; a second signal cuts the wait short. See [Runtime and concurrency](/concepts/runtime-and-concurrency.md).

## Control plane

- **Layered configuration** — a file layer plus an optional `conduitctl` overlay compose into the effective config and an immutable runtime snapshot. See [Configuration model](/control-plane/configuration-model.md).
- **Hot reload** — reload from disk with **SIGHUP** or `conduitctl reload`; new queries pick up the new snapshot while in-flight transactions keep the policy they started with.
- **Optional gRPC API and `conduitctl`** — `validate` (offline), plus `apply`, `export`, `reload`, `trace`, and `health` when the control plane is enabled. Apply supports merge (default), `--replace`, and `--clear`. See [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md).
- The dataplane always handles DNS; the control plane is entirely optional.

## Security

- **Control-plane API keys** for `conduitctl` authentication. See [API keys](/security/api-keys.md).
- **mTLS** on the gRPC control listener with client-certificate requirements. See [mTLS](/security/mtls.md).

## Observability

Observation runs **off** the DNS query path — export backlog drops data rather than delaying client responses.

- **Metrics** — Prometheus scrape and optional OTLP push with equivalent semantics, `minimal` and `full` profiles, and backend-health gauges. See [Metrics](/observability/metrics.md) and [Built-in metrics](/observability/built-in-metrics.md).
- **Pipeline tracing** — per-query phase timelines retrievable via `conduitctl trace` / `GetTrace`. See [Tracing](/observability/tracing.md).
- **Event export** — dnstap and structured event sinks with filters, selectors (including `answer_source` and `cache_instance`), extra fields, and sampling. See [Event export](/observability/event-export.md).
- **Structured logging** — lifecycle, reload, and control-plane events by default, with per-query summaries at `debug`. See [Logging](/observability/logging.md).

## Interop and correctness

- A published **correctness matrix**, organized by publisher, records tested behavior against multiple third-party DNS implementations (forwarding, cache, ordered rules, backend health, and per-service response shapes). Results and last-tested provenance are on the [Interop overview](/interop/index.md).
- The harness can be reproduced locally against a pinned container image; it is not run by GitHub Actions. See [Interop correctness matrix](/interop/correctness-matrix.md).

## Platform, packaging, and distribution

- **Ubuntu 22.04 / 24.04 (amd64)** target. Each release publishes stripped and unstripped tarballs, production and debug Debian packages, `SHA256SUMS`, an SPDX **SBOM**, and a container image on GHCR (reference and digest recorded in `conduit-<version>.image-digest.txt`). See [Install and run](/getting-started/install-and-run.md).
- Every artifact ships three binaries: **`conduit`** (dataplane), **`conduitctl`** (control-plane CLI), and **`conduit-dnstap-tracer`** (dnstap troubleshooting utility). The Debian package installs a `conduit` system user and a systemd unit with `CAP_NET_BIND_SERVICE`.
- Licensed under **Apache 2.0** with DCO sign-off for contributions.

## Getting started

1. [Install and run](/getting-started/install-and-run.md) — packages, container image, and first start.
2. [Minimal configuration](/getting-started/minimal-configuration.md) — smallest runnable YAML (`listeners`, `pools`).
3. [First query](/getting-started/first-query.md) — send a test query through Conduit.
4. [Architecture and packet path](/concepts/architecture-and-packet-path.md) — how one query moves through the pipeline.

---

[All changes in this release](https://github.com/egon1024/DNSConduit/releases/tag/1.0.0) (automated pull request list).
