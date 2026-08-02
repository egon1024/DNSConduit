# Tuning evidence (studies)

Comparative case studies from the performance harness: operator questions,
controlled axes, and same-host comparisons against baselines on a single
reference host. These are **not** service-level objectives. Reproduce before sizing
locally.

**Prefer this hub for decisions.** Start from
[Performance findings](/performance/index.md#findings) for a short synthesis, then
open the matching study below. The [reference results](/performance/reference.md)
page is the dense chart warehouse; [methodology](/performance/methodology.md)
explains how cells were measured.

## If you are deciding

### Runtime & workers

| If you are deciding… | Start here |
| --- | --- |
| [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) under fast / slow upstreams | [Sync vs split_io](/performance/studies/sync-vs-split-io.md) |
| More UDP ingress threads on **[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)** | [Ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md) |
| More I/O workers on **[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)** (ingress fixed) | [I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md) |
| Whether raising all worker pools together helps | [Split_io bulk topology](/performance/studies/split-io-thread-bulk.md) |
| Warm answer-cache vs forwarding throughput | [Cache hit vs forward](/performance/studies/cache-hit-vs-forward.md) |

### Observability tax

| If you are deciding… | Start here |
| --- | --- |
| `metrics.base` minimal vs standard scrape cost | [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md) |
| Collect vs emit (recording vs export) | [Metrics collect vs emit](/performance/studies/metrics-collect-vs-emit.md) |
| OTLP metrics push cost vs off / scrape | [OTLP tax under load](/performance/studies/otlp-tax-under-load.md) |
| Scrape tax when already on **[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)** | [Metrics scrape (split_io)](/performance/studies/metrics-scrape-split-io.md) |
| Frequent Prometheus scrape under load | [Aggressive scrape cadence](/performance/studies/metrics-scrape-hammer.md) |
| Dnstap off / sampled / fuller emit cost | [Dnstap emit tax](/performance/studies/dnstap-emit-tax.md) |
| Standard scrape **and** fuller dnstap together | [Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md) |
| Process log level warn vs debug | [Logging verbosity tax](/performance/studies/logging-verbosity-tax.md) |
| Pipeline tracing on vs off | [Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md) |

### Lifecycle

| If you are deciding… | Start here |
| --- | --- |
| Shutdown drain complete / budgeted / minimal | [Drain policy under slow](/performance/studies/drain-policy-under-slow.md) |

After the study: follow its **Related guides** into config and concepts, then
[reproduce](/performance/reproduce.md) the same study (`--study …`) on your
binary if the decision is load-sensitive.

## Related

- [Performance hub](/performance/index.md) — [Findings](/performance/index.md#findings)
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) —
  [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
  [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) models and worker roles
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [Reference results](/performance/reference.md) (warehouse charts)
- [Methodology](/performance/methodology.md)
- [Reproduce against a binary](/performance/reproduce.md)
