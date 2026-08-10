# Performance

Directional performance evidence for DNS Conduit: runtime model tradeoffs,
worker sizing, observability tax, and shutdown drain under load. Published
figures are **same-host comparisons** on a **single reference host**, not
service-level objectives.

## Findings

Short directional takeaways from published studies. Each bullet is a
**same-host relative** result on a single reference host — not a portable
capacity target or SLO. Absolute QPS on study pages is lab detail only; remeasure
on your hardware before sizing.

- **Runtime model** — Under a fast upstream, [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) reaches about **1.9×** the QPS of [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) (~141k vs ~74k). → [Sync vs split_io](/performance/studies/sync-vs-split-io.md)
- **Sync ingress sizing** — More ingress workers raise throughput under fast forward (~**38k → 74k → 146k → 240k** from 1→2→4→8); gains stay large at the top of that range on this lab. → [Ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md)
- **Answer cache** — When nearly every query hits, a warm cache path is about **3.5×** forward-fast QPS on this lab — a ceiling, not a forecast of your hit rate. → [Cache hit vs forward](/performance/studies/cache-hit-vs-forward.md)
- **Memory vs LMDB (warm)** — Warm LMDB costs about **6%** QPS versus warm memory on this lab (~311k vs ~329k) under sync cache_hit — read-mostly after warm, not churn. → [Memory vs LMDB warm cache_hit](/performance/studies/memory-vs-lmdb-cache-hit.md)
- **Memory vs LMDB (churn)** — Under matched high churn (ingress-8), LMDB **`sync: full`** costs about **99%** QPS versus memory (~3k vs ~232k; roughly **83.9×**). **`no_meta`** is only about **1.6×** `full` (~4k); **`periodic`** (`sync_interval` 1s) is about **43.8×** `no_meta` (~192k, about **17%** versus memory); **`none`** is nearby (~177k, about **24%** versus memory). Hit rates stay similar; pick sync mode from the durability decision tree, not the QPS chart alone. → [Memory vs LMDB high-churn cache](/performance/studies/memory-vs-lmdb-cache-churn.md)
- **Metrics scrape** — Versus observability off, standard scrape costs about **9%** QPS on the sync ladder in this median refresh; collect carries most metrics cost, with emit a thin band. → [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md), [Collect vs emit](/performance/studies/metrics-collect-vs-emit.md)
- **Logging and tracing** — Debug logging costs about **5%** versus warn on this median; full pipeline tracing (~100% sample) costs about **37%** versus observability off. Keep tracing for diagnosis windows, not standing production. → [Logging verbosity tax](/performance/studies/logging-verbosity-tax.md), [Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md)
- **Dnstap and OTLP** — Sampled dnstap about **6%**, fuller emit about **10%**; scrape+dnstap together about **15%**. OTLP push about **8%** — same band as standard scrape. → [Dnstap emit tax](/performance/studies/dnstap-emit-tax.md), [Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md), [OTLP tax under load](/performance/studies/otlp-tax-under-load.md)
- **Shutdown drain** — Under slow upstream, complete drain is ~**114 ms**; budgeted ~**164 ms**; minimal ~**63 ms**, with complete recording the most in-flight client failures. Pick a policy for your restart window on purpose. → [Drain policy under slow](/performance/studies/drain-policy-under-slow.md)

Full comparisons, charts, and omitted-member notes live under
[Tuning evidence (studies)](/performance/studies/index.md).

## Start here if deploying

Three steps for first sizing on **your** hardware. Published figures stay
same-host comparisons on a single reference host — not capacity SLOs.

1. **Pick a runtime** — read the [runtime model finding](#findings) (~1.9×
   `split_io` vs `sync` under a fast upstream), then the
   [Sync vs split_io](/performance/studies/sync-vs-split-io.md) study.
2. **Size metrics scrape** — read the [metrics scrape finding](#findings)
   (~9% standard vs off on the sync ladder), then
   [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md) and
   [Operator metrics bases](/guides/operator-metrics-bases.md).
3. **Remeasure one study** — replay a study against your binary with
   [Reproduce against a binary](/performance/reproduce.md) (`--study …`) before
   locking in worker counts or observability posture.

## How to use this section

1. **Decide** — skim [Findings](#findings), then open the matching
   [study](/performance/studies/index.md) for charts and takeaways.
2. **Interpret** — [Methodology](/performance/methodology.md) explains load shapes,
   how load is applied, and how to read published numbers.
3. **Remeasure** — [Reproduce against a binary](/performance/reproduce.md) on your
   hardware before sizing decisions.
4. **Look up raw numbers (optional)** — When you need the full charts or CSV
   rows, open [Reference results](/performance/reference.md). To see what a
   specific table row means, follow its link into
   [Scenarios](/performance/scenarios.md).

Already know which tradeoff you are weighing? Jump to the
[decision map on the studies hub](/performance/studies/index.md#if-you-are-deciding).

## Disclaimer

Published reference results were measured on a single reference host
(`maintainer-ws-1`). Charts and studies are **same-host comparisons** — each
figure contrasts configurations against baselines taken on that host under the
same load recipe. They are **not** capacity guarantees or portable cross-host
SLOs. Reproduce with the harness against **your** Conduit binary and hardware
before making local sizing decisions. Do not treat absolute QPS as transferable
across machines.

## In this section

- [Findings](#findings) — short directional takeaways
- [Start here if deploying](#start-here-if-deploying) — runtime → metrics tax → remeasure
- [Tuning evidence (studies)](/performance/studies/index.md) — decision evidence by category
- [Methodology](/performance/methodology.md) — how to interpret published numbers
- [Reproduce against a binary](/performance/reproduce.md) — remeasure on your hardware
- [Reference results](/performance/reference.md) — dense chart/CSV warehouse (optional)
- [Scenarios](/performance/scenarios.md) — row-level glossary (deep links from tables)

## Related

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), workers, drain
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [DNS answer cache](/guides/dns-answer-cache.md)
- [Event export](/observability/event-export.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Shutdown config](/reference/config-schema/shutdown.md)
- [OTLP metrics push smoke](/guides/otlp-metrics-push.md)
