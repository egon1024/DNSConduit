## At a glance

- **sync ingress-8 high-churn (memory vs LMDB sync modes):** `full` costs about **99%** QPS versus `memory` (~3k vs ~268k); `no_meta` costs about **98%** QPS versus `memory` (~5k vs ~268k); `none` costs about **40%** QPS versus `memory` (~160k vs ~268k).
- **Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes):** `full` is about **14%** lower hit rate than `memory` (~42.8% vs ~49.7%); `no_meta` is about **6%** lower hit rate than `memory` (~46.5% vs ~49.7%); `none` is about **1%** higher hit rate than `memory` (~50.1% vs ~49.7%).
- **Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes):** `full` ≈ ~2.7 ms vs `memory` ~0.0005 ms; `no_meta` ≈ ~1.9 ms vs `memory` ~0.0005 ms; `none` is about **22.4×** `memory` (~0.011 ms vs ~0.0005 ms).
