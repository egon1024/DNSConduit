# Guides

This section collects end-to-end walkthroughs for common operator tasks. Each guide is self-contained: concrete YAML, commands, and what to verify. Conceptual detail and field reference are described in [Control plane](/control-plane/index.md), [Policy & routing](/policy-routing/index.md), and [Observability](/observability/index.md).

**Prerequisites for most guides:** Conduit installed ([Install and run](/getting-started/install-and-run.md)), a runnable baseline config ([Minimal configuration](/getting-started/minimal-configuration.md)), and — where noted — the [control plane](/glossary/index.md#control-plane) enabled with `control.listen_address`.

Primary lab configs for guides that ship a full runnable sample are also installed with the production package and tarball under **`/usr/share/doc/conduit/examples/`** (or `examples/` in the tarball). See that directory’s `README.md` for the map from path to guide.

After your first successful query ([First query](/getting-started/first-query.md)), work top-down for a typical path — reliability → policy → live change → observability → capacity — or jump to the task you need.

### Routing and reliability

| Guide | What you practice |
|-------|-------------------|
| [Backend health](/guides/backend-health.md) | Enable probes, watch a dead backend go down, practice `conduitctl health` drain/resume |
| [DNS answer cache](/guides/dns-answer-cache.md) | Enable in-memory caching, hit/miss path, `on_hit` and metrics tradeoffs |
| [Dual-stack forwarding](/guides/dual-stack-forwarding.md) | Global and per-pool egress sources, rules, and Rhai overrides |

### Policy

| Guide | What you practice |
|-------|-------------------|
| [Rule action order](/guides/rule-action-order.md) | Soft vs hard drop, `clear_drop`, action-list order, request `set_retry_pool` stash |
| [Declarative failover](/guides/declarative-failover.md) | SERVFAIL / timeout retry to another pool or backend without Rhai |
| [Rhai policy](/guides/rhai-policy.md) | CSV blocklist drop with custom user metric; CSV-driven pool routing |

### Control plane

| Guide | What you practice |
|-------|-------------------|
| [Control plane workflows](/guides/control-plane-workflows.md) | Reload from disk, temporary `conduitctl apply` overlays, export, and when to restart |

### Observability

| Guide | What you practice |
|-------|-------------------|
| [Metrics and tracing](/guides/metrics-and-tracing.md) | Prometheus scrape, counters after traffic, `conduitctl trace` |
| [Operator metrics bases](/guides/operator-metrics-bases.md) | **`minimal`** vs **`standard`** built-in metrics on the same traffic |
| [Metrics beyond bases](/guides/metrics-beyond-bases.md) | Categories, collect vs emit, granularity, live overlay and scrape rebind |
| [Event export and dnstap](/guides/event-export-dnstap.md) | dnstap sinks with `conduit-dnstap-tracer` |

### Performance

| Guide | What you practice |
|-------|-------------------|
| [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) | Choosing `sync` vs `split_io`, sizing ingress/policy/I/O workers, the slot pool, and `reuse_port` |

## Related topics

- [Getting started](/getting-started/index.md) — install, minimal config, first query
- [Control plane](/control-plane/index.md) — configuration model, reload and export reference
- [Troubleshooting](/troubleshooting/index.md) — symptom tables when something does not behave as expected
