## At a glance

- **sync ingress-8 high-churn (memory vs LMDB):** `lmdb` costs about **99%** QPS versus `memory` (~3k vs ~268k).
- **Cache hit rate — sync ingress-8 high-churn (memory vs LMDB):** `lmdb` is about **6%** lower hit rate than `memory` (~46.8% vs ~49.7%).
- **Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB):** `lmdb` ≈ ~3.0 ms vs `memory` ~0.0005 ms.
