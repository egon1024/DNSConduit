!!! note "Reading forward_slow cells — saturation against a 50 ms backend"
    [`forward_slow`](/performance/methodology.md#load-shapes) cells offer far more
    concurrency than a runtime that blocks on upstream latency can absorb, which is
    the point: they show what happens to each runtime model when the backend is slow
    and the client keeps asking. Read both columns together. A runtime that
    multiplexes in-flight queries answers near the upstream delay itself; a runtime
    that occupies a worker for the whole round trip reports a small fraction of that
    throughput and an average latency of seconds, because queries wait in Conduit
    rather than at the backend. Every published cell here is measured on a stub
    upstream that stays well clear of saturation and is checked for successful
    answers, so the numbers are Conduit's behavior, not the harness reaching its
    limit. See
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps)
    and [Only successful answers count](/performance/methodology.md#only-successful-answers-count).
