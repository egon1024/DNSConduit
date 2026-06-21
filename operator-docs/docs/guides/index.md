# Guides

End-to-end walkthroughs for common operator tasks. Each guide is self-contained: concrete YAML, commands, and what to verify. Conceptual detail and field reference live in [Control plane](/control-plane/index.md), [Policy & routing](/policy-routing/index.md), and [Observability](/observability/index.md).

**Prerequisites for most guides:** Conduit installed ([Install and run](/getting-started/install-and-run.md)), a runnable baseline config ([Minimal configuration](/getting-started/minimal-configuration.md)), and — where noted — the [control plane](/glossary/index.md#control-plane) enabled with `control.listen_address`.

| Guide | What you practice |
|-------|-------------------|
| [Control plane workflows](/guides/control-plane-workflows.md) | Reload from disk, temporary `conduitctl apply` overlays, export, and when to restart |
| [Dual-stack forwarding](/guides/dual-stack-forwarding.md) | Global and per-pool egress sources, rules, and Rhai overrides |
| [Metrics and tracing](/guides/metrics-and-tracing.md) | Prometheus scrape, counters after traffic, `conduitctl trace` |
| [Event export and dnstap](/guides/event-export-dnstap.md) | dnstap sinks with `conduit-dnstap-tracer` |
| [Operator metrics profiles](/guides/operator-metrics-profiles.md) | **`minimal`** vs **`full`** built-in metrics on the same traffic |

After your first successful query, start with [First query](/getting-started/first-query.md), then pick a guide that matches your next task.

## Related topics

- [Getting started](/getting-started/index.md) — install, minimal config, first query
- [Control plane](/control-plane/index.md) — configuration model, reload and export reference
- [Troubleshooting](/troubleshooting/index.md) — symptom tables when something does not behave as expected
