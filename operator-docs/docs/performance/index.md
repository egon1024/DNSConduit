# Performance

Directional performance evidence for DNS Conduit: runtime model tradeoffs,
worker sizing, observability tax, and shutdown drain under load. Published
figures are **same-host comparisons** on a **named maintainer lab host
profile**, not service-level objectives.

## How to use this section

1. **Decide a knob** — start with [Tuning evidence (studies)](/performance/studies/index.md)
   (question-shaped comparisons and takeaways).
2. **Confirm how numbers were made** — [Methodology](/performance/methodology.md)
   (load shapes, dnsperf concurrency model, promote vs docs render).
3. **Remeasure locally** — [Reproduce against a binary](/performance/reproduce.md)
   before sizing on your hardware.
4. **Optional warehouse** — [Reference results](/performance/reference.md) is the
   dense chart catalog; [Scenarios](/performance/scenarios.md) describe each row.

If you already know the decision, jump from the
[use-case map on the studies hub](/performance/studies/index.md#if-you-are-deciding).

## Disclaimer

Published reference results were measured on one named maintainer workstation
profile (`maintainer-ws-1`). Charts and studies are **same-host comparisons** —
each figure contrasts configurations against baselines taken on that host under
the same load recipe. They are **not** capacity guarantees or portable cross-host
SLOs. Reproduce with the harness against **your** Conduit binary and hardware
before making local sizing decisions. Do not treat absolute QPS as transferable
across machines.

## In this section

- [Tuning evidence (studies)](/performance/studies/index.md) — comparative case studies for feature and runtime decisions (**start here**)
- [Methodology](/performance/methodology.md) — suites, loadgen model, drain vocabulary, load shapes, promote vs docs render
- [Reproduce against a binary](/performance/reproduce.md) — Python harness + Docker dnsperf (no rustc required for suite replay)
- [Reference results](/performance/reference.md) — curated tables, static SVG charts, per-chart CSV downloads (warehouse)
- [Scenarios](/performance/scenarios.md) — intent and axes for each curated scenario (linked from reference rows)

## Related

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), workers, drain
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [DNS answer cache](/guides/dns-answer-cache.md)
- [Event export](/observability/event-export.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Shutdown config](/reference/config-schema/shutdown.md)
- [OTLP metrics push smoke](/guides/otlp-metrics-push.md)
