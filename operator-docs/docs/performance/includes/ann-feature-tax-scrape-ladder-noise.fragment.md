!!! note "Metrics scrape ladder — published forward_fast recipe"
    These scrape-ladder cells use the same published [`forward_fast`](/performance/methodology.md#load-shapes)
    dnsperf recipe as other scale / feature-tax fast cells (clients 16, threads 8,
    max outstanding 2000) and are the **median of 3 independent rounds**. That is
    the shared publish recipe so achieved QPS reflects Conduit capacity rather than
    a thin outstanding window — not a scrape-ladder-only workaround. See
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps).
    Remeasure locally with the same recipe before sizing.
