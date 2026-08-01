!!! note "Curated performance reference — publish-set remeasure"
    Promoted reference for the performance harness curated publish-set:
    [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
    [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) under
    [`forward_fast`](/performance/methodology.md#load-shapes) and
    [`forward_slow`](/performance/methodology.md#load-shapes) (separate charts per
    shape), [`cache_hit`](/performance/methodology.md#load-shapes) and
    topology-heavy scale extras, worker counts under `forward_slow`, the three
    shutdown drain policies under `forward_slow`, and metrics / dnstap / OTLP
    feature-tax configurations. Published `forward_fast` / `cache_hit` cells use
    the elevated dnsperf outstanding recipe, three-round median merge, and a CPU
    `performance` governor gate on the lab host (see
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps)
    and [Lab profiles](/performance/methodology.md#lab-profiles)). Numbers are
    same-host comparisons on the named maintainer workstation profile — not
    capacity SLOs.
