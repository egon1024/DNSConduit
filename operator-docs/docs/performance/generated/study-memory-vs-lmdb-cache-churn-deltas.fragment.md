## At a glance

- **sync high-churn cache (memory vs LMDB):** `lmdb` costs about **99%** QPS versus `memory` (~1k vs ~92k).
- **Cache hit rate — sync high-churn (memory vs LMDB):** `lmdb` costs about **46%** QPS versus `memory` (~26 QPS vs ~49 QPS).
