## At a glance

- **collect vs emit (forward_fast):** `metrics_collect_only` costs about **5%** QPS versus `metrics_no_collect` (~70k vs ~73k); `metrics_collect_emit` costs about **6%** QPS versus `metrics_no_collect` (~69k vs ~73k).
