!!! warning "forward_slow scale/ladder cells — stressed / inconclusive for ranking"
    Several promoted [`forward_slow`](/performance/methodology.md#load-shapes) scale and
    worker-ladder cells show very low achieved QPS with high dnsperf query loss. Under
    the published load model (timed window, no offered-QPS cap, dnsperf default max
    outstanding ≈ 100), an artificially delayed upstream fills the outstanding window
    quickly, so these charts are poor for ranking
    [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
    [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) or worker counts.
    Prefer [`forward_fast`](/performance/methodology.md#load-shapes) cells for clean
    same-host deltas; treat `forward_slow` here as a stress recipe until a
    publish-quality remeasure replaces the cells. See
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps).
