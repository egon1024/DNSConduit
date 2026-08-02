!!! note "Curated performance reference — publish-set remeasure"
    Promoted reference for the performance harness curated publish-set:
    [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
    [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) under
    [`forward_fast`](/performance/methodology.md#load-shapes), sync ingress ladders
    under both forward shapes, [`cache_hit`](/performance/methodology.md#load-shapes),
    the three shutdown drain policies under `forward_slow`, and metrics / dnstap /
    OTLP feature-tax configurations. Published cells share one elevated dnsperf
    outstanding recipe, are the median of three rounds, must clear the
    successful-answer check, and run on a lab host gated to the CPU `performance`
    governor (see
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps),
    [Only successful answers count](/performance/methodology.md#only-successful-answers-count),
    and [Lab profiles](/performance/methodology.md#lab-profiles)).
    `split_io` cells under [`forward_slow`](/performance/methodology.md#load-shapes)
    and the topology-heavy bulk pole did not clear the answer gate in this
    remeasure (SERVFAIL storms) and are omitted until a clean measurement is
    available. Numbers are same-host comparisons on the named maintainer
    workstation profile — not capacity SLOs.
