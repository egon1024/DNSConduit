## At a glance

- **collect vs emit (forward_fast):** `metrics_collect_only` is about **1.0×** `metrics_no_collect` (~70k vs ~68k); `metrics_collect_emit` costs about **6%** QPS versus `metrics_no_collect` (~64k vs ~68k).
