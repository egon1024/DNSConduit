# Tuning evidence (studies)

Comparative case studies from the performance harness: operator questions,
controlled axes, and same-host comparisons against baselines on one named
lab profile. These are **not** service-level objectives. Reproduce before sizing
locally.

**Prefer this hub for decisions.** The [reference results](/performance/reference.md)
page is the dense chart warehouse; [methodology](/performance/methodology.md)
explains how cells were measured.

## If you are deciding

| If you are deciding… | Start here |
| --- | --- |
| [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) under fast / slow upstreams | [Sync vs split_io](/performance/studies/sync-vs-split-io.md) |
| More UDP ingress threads on **[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)** | [Ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md) |
| More I/O workers on **[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)** (ingress fixed) | [I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md) |
| Whether raising all worker pools together helps | [Split_io bulk topology](/performance/studies/split-io-thread-bulk.md) |
| `metrics.base` minimal vs standard scrape cost | [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md) |
| Collect vs emit (recording vs export) | [Metrics collect vs emit](/performance/studies/metrics-collect-vs-emit.md) |
| OTLP metrics push cost vs off / scrape | [OTLP tax under load](/performance/studies/otlp-tax-under-load.md) |
| Scrape tax when already on **[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)** | [Metrics scrape (split_io)](/performance/studies/metrics-scrape-split-io.md) |
| Frequent Prometheus scrape under load | [Aggressive scrape cadence](/performance/studies/metrics-scrape-hammer.md) |
| Dnstap off / sampled / fuller emit cost | [Dnstap emit tax](/performance/studies/dnstap-emit-tax.md) |
| Standard scrape **and** fuller dnstap together | [Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md) |
| Process log level warn vs debug | [Logging verbosity tax](/performance/studies/logging-verbosity-tax.md) |
| Pipeline tracing on vs off | [Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md) |
| Shutdown drain complete / budgeted / minimal | [Drain policy under slow](/performance/studies/drain-policy-under-slow.md) |
| Warm answer-cache vs forwarding throughput | [Cache hit vs forward](/performance/studies/cache-hit-vs-forward.md) |

After the study: follow its **Related guides** into config and concepts, then
[reproduce](/performance/reproduce.md) the same study (`--study …`) on your
binary if the decision is load-sensitive.

<!-- perf-studies-index:start -->
_Generated index 2026-08-01T03:04:34Z from the study catalog (evidence from committed reference JSON)._

| Study | Question |
| --- | --- |
| [Sync vs split_io under paired load shapes](/performance/studies/sync-vs-split-io.md) | When does split_io beat sync for forwarding throughput and latency? |
| [Ingress concurrency under sync](/performance/studies/ingress-concurrency-sync.md) | How does raising sync ingress workers change throughput under a slow upstream? |
| [I/O workers vs fixed ingress under split_io](/performance/studies/io-vs-ingress-split.md) | With receive and policy threads fixed on split_io, does adding I/O workers help under a slow upstream? |
| [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md) | What does enabling richer Prometheus scrape metrics cost under forward_fast? |
| [Metrics collect vs emit](/performance/studies/metrics-collect-vs-emit.md) | Is metrics cost dominated by hot-path collect, or by scrape emit? |
| [OTLP tax under load](/performance/studies/otlp-tax-under-load.md) | What does OTLP metrics push cost versus observability off under forward_fast? |
| [Metrics scrape tax under split_io](/performance/studies/metrics-scrape-split-io.md) | What does standard scrape cost versus obs-off when the runtime is split_io? |
| [Aggressive scrape cadence under load](/performance/studies/metrics-scrape-hammer.md) | What does frequent Prometheus scraping during load cost versus listener-only scrape? |
| [Dnstap emit tax](/performance/studies/dnstap-emit-tax.md) | What does sampled vs fuller dnstap emission cost versus dnstap off? |
| [Combined metrics + dnstap tax](/performance/studies/metrics-dnstap-combined-tax.md) | What does turning on standard scrape and fuller dnstap together cost? |
| [Logging verbosity tax](/performance/studies/logging-verbosity-tax.md) | What does raising process log level from warn to debug cost under forward_fast? |
| [Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md) | What does enabling full pipeline tracing cost under forward_fast? |
| [Drain policy under slow upstream](/performance/studies/drain-policy-under-slow.md) | How do complete, budgeted, and minimal drain policies behave under forward_slow load? |
| [Cache hit vs forward_fast](/performance/studies/cache-hit-vs-forward.md) | How much does a warm lookup cache change throughput versus forward_fast under sync? |
| [Split_io bulk thread topology](/performance/studies/split-io-thread-bulk.md) | Does raising ingress, policy, and I/O worker counts together always help under forward_fast? |
<!-- perf-studies-index:end -->

## Related

- [Performance hub](/performance/index.md)
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) —
  [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
  [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) models and worker roles
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [Reference results](/performance/reference.md) (warehouse charts)
- [Methodology](/performance/methodology.md)
- [Reproduce against a binary](/performance/reproduce.md)
