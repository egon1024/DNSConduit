---
toc_depth: 3
toc_collapsible: true
---

# Performance scenarios

Each curated performance scenario is a named lab setup: what Conduit is doing,
what kind of load it faces, and which knobs stay fixed. Table rows on
[reference results](/performance/reference.md) and study evidence deep-link here.

This page is a glossary for those links — not the primary decision surface. Start
from [Performance findings](/performance/index.md#findings) or
[Tuning evidence (studies)](/performance/studies/index.md).

Recurring terms such as [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) /
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) and load shapes
[`forward_fast`](/performance/methodology.md#load-shapes) /
[`forward_slow`](/performance/methodology.md#load-shapes) /
[`cache_hit`](/performance/methodology.md#load-shapes) are defined in
[methodology](/performance/methodology.md#load-shapes) and
[runtime and concurrency](/concepts/runtime-and-concurrency.md).

<!-- perf-scenarios-body:start -->
## feature_tax

<div class="perf-scenario" markdown="1">

### feature-tax-dnstap-off-forward-fast

Baseline for dnstap cost: forwarding to a fast stub upstream with dnstap and event sinks disabled.

**Notes:** Compare with the sampled and full dnstap cells in the same study.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`dnstap_off`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-dnstap-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-dnstap-sampled-forward-fast

Measures dnstap cost when only about 10% of response frames are emitted to a lab receiver, still under a fast stub upstream.

**Notes:** Middle cell between dnstap-off and full dnstap emit. Requires the lab dnstap tracer.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`dnstap_sampled`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-dnstap-sampled.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-dnstap-full-forward-fast

Measures how much throughput changes when Conduit emits full dnstap event frames (query, response, and retry) to a lab receiver while forwarding to a fast stub upstream.

**Notes:** Part of the dnstap cost series with dnstap-off and dnstap-sampled. Requires the lab dnstap tracer.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`dnstap_full`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-dnstap-full.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-logging-warn-forward-fast

Measures quieter logging: metrics are off and logging.level is warn while forwarding to a fast stub upstream.

**Notes:** Paired with the logging-debug cell. Named for the logging axis — it is not the shared metrics-off baseline used by other feature_tax studies.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`logging_warn`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-logging-warn.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-logging-debug-forward-fast

Measures the cost of verbose logging: metrics are off and logging.level is debug while forwarding to a fast stub upstream.

**Notes:** Paired with feature-tax-logging-warn-forward-fast for the logging verbosity comparison.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`logging_debug`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-logging-debug.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-no-collect-forward-fast

Hot-path metric categories neither record nor export (no collect) under a fast stub upstream.

**Notes:** Paired with collect-only and collect+emit.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_no_collect`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-no-collect.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-collect-only-forward-fast

Records hot-path metrics in process but does not scrape or push them (collect on, emit off) under a fast stub upstream.

**Notes:** Paired with collect+emit and no-collect.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_collect_only`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-collect-only.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-collect-emit-forward-fast

Records hot-path metrics and exports them (collect and emit) under a fast stub upstream, using the standard metrics base.

**Notes:** Paired with collect-only and no-collect for the collect-versus-emit comparison.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_collect_emit`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-collect-emit.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-off-forward-fast

Observability-off baseline under a fast stub upstream: metrics disabled and no dnstap sinks.

**Notes:** Shared baseline for several feature_tax comparisons (scrape cost, combined surfaces, tracing, and similar).

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_off`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-forward-fast

Prometheus scrape with the standard metrics base (collect and emit) while forwarding to a fast stub upstream.

**Notes:** High end of the scrape cost series; also the collect+emit pole in collect comparisons that reuse this cell.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_standard_scrape`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-standard-scrape.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-dnstap-full-forward-fast

Combined observability cost: standard Prometheus scrape plus full dnstap emit under a fast stub upstream.

**Notes:** Used in studies that stack scrape and dnstap together.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_standard_dnstap_full`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-standard-dnstap-full.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-hammer-forward-fast

Same standard scrape posture as the listener-only scrape cell, but a lab client hammers /metrics about every 100 ms during the run.

**Notes:** Shows aggressive scrape cadence versus ordinary scrape traffic on the listener.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_standard_scrape_hammer`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-standard-scrape.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-off-scrape-ladder-forward-fast

Observability-off baseline for the metrics scrape cost series, using the same elevated dnsperf in-flight window as the other cells in that series.

**Notes:** Not the shared metrics-off cell used by other feature_tax studies — this one matches the scrape-series loadgen recipe.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_off`, `ingress_workers`=`2`, `loadgen_recipe`=`elevated_outstanding`

**How it was run:** config `feature-tax-metrics-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-minimal-scrape-ladder-forward-fast

Same minimal metrics scrape posture as the standard minimal-scrape cell, run with a larger dnsperf in-flight window so the scrape cost series is not limited by the load generator.

**Notes:** Published recipe for the metrics scrape cost series; pair with the off and standard cells that share this elevated outstanding window.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_minimal_scrape`, `ingress_workers`=`2`, `loadgen_recipe`=`elevated_outstanding`

**How it was run:** config `feature-tax-metrics-minimal-scrape.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-ladder-forward-fast

Standard metrics scrape under the elevated dnsperf in-flight window used by the scrape cost series.

**Notes:** Pair with the off and minimal cells that share this loadgen recipe.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_standard_scrape`, `ingress_workers`=`2`, `loadgen_recipe`=`elevated_outstanding`

**How it was run:** config `feature-tax-metrics-standard-scrape.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-off-split-io-forward-fast

Observability-off baseline on the split_io runtime while forwarding to a fast stub upstream.

**Notes:** Paired with feature-tax-metrics-standard-scrape-split-io-forward-fast for scrape cost on split_io.

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_off`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**How it was run:** config `feature-tax-metrics-off-split-io.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-split-io-forward-fast

Standard Prometheus scrape on the split_io runtime while forwarding to a fast stub upstream.

**Notes:** Paired with feature-tax-metrics-off-split-io-forward-fast for scrape cost on split_io.

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_standard_scrape`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**How it was run:** config `feature-tax-metrics-standard-scrape-split-io.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-otlp-push-forward-fast

Measures the cost of pushing metrics over OTLP HTTP while forwarding to a fast stub upstream.

**Notes:** Requires conduit-otlp-metrics-tracer. The lab receiver also records otlp_accepts / otlp_failures as secondary signals.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_otlp_push`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-otlp-push.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-tracing-on-forward-fast

Metrics stay off while pipeline tracing is enabled (query type A at 100% sample) under a fast stub upstream.

**Notes:** Paired with feature-tax-metrics-off-forward-fast.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`tracing_on`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-tracing-on.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-minimal-scrape-forward-fast

Prometheus scrape with the minimal metrics base (collect and emit) while forwarding to a fast stub upstream.

**Notes:** Middle cell between metrics-off and standard scrape.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_minimal_scrape`, `ingress_workers`=`2`

**How it was run:** config `feature-tax-metrics-minimal-scrape.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

## lifecycle

<div class="perf-scenario" markdown="1">

### lifecycle-cold-start

Wall-clock time from process start until Conduit returns its first successful DNS answer.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_off`, `ingress_workers`=`2`

**How it was run:** config `lifecycle-cold-start.yml`; upstream `fast` (fast stub upstream); loadgen `none`.

</div>

<div class="perf-scenario" markdown="1">

### lifecycle-config-apply

How long a conduitctl apply takes for a sparse overlay that only changes the logging level.

**Notes:** Small lifecycle cell for local runs; not required in the published reference set.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `obs_posture`=`metrics_off`, `ingress_workers`=`2`

**How it was run:** config `lifecycle-config-apply-base.yml`; upstream `fast` (fast stub upstream); loadgen `none`.

</div>

## scale

<div class="perf-scenario" markdown="1">

### scale-sync-forward-fast

Throughput for the sync runtime while forwarding to a fast stub upstream.

**Notes:** Thin published pair with scale-split-io-forward-fast.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`

**How it was run:** config `scale-sync-obs-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-cache-hit

Warm memory-cache hits on the sync runtime: the harness fills the cache before dnsperf, then measures near-100% hits against a small answer set.

**Notes:** Read-mostly after warm — not a high-churn fill/evict shape. Optional for the published reference set; compare with scale-sync-lmdb-cache-hit via the memory-vs-lmdb-cache-hit study.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`cache_hit`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`, `cache_backend`=`memory`

**How it was run:** config `scale-sync-cache-hit.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-1-forward-fast

Sync runtime with one ingress worker under a fast stub upstream.

**Notes:** Uses an elevated dnsperf outstanding window so achieved QPS reflects Conduit ingress capacity rather than the load generator. Part of the ingress concurrency series (1/2/4/8).

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `ingress_workers`=`1`

**How it was run:** config `scale-sync-ingress-1-obs-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-4-forward-fast

Sync runtime with four ingress workers under a fast stub upstream.

**Notes:** Elevated dnsperf outstanding window so QPS reflects ingress capacity. Part of the ingress concurrency series (1/2/4/8).

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `ingress_workers`=`4`

**How it was run:** config `scale-sync-ingress-4-obs-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-8-forward-fast

Sync runtime with eight ingress workers under a fast stub upstream.

**Notes:** Elevated dnsperf outstanding window so QPS reflects ingress capacity. Part of the ingress concurrency series (1/2/4/8).

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `ingress_workers`=`8`

**How it was run:** config `scale-sync-ingress-8-obs-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-1-forward-slow

Sync runtime with one ingress worker under a slow stub upstream.

**Notes:** Paired with ingress 2/4/8 cells for the ingress-concurrency-sync study.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`1`

**How it was run:** config `scale-sync-ingress-1-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-forward-slow

Throughput for the sync runtime while every upstream answer is held for 50 ms (forward_slow).

**Notes:** Paired with scale-split-io-forward-slow for relative comparison.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`

**How it was run:** config `scale-sync-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-4-forward-slow

Sync runtime with four ingress workers under a slow stub upstream.

**Notes:** Paired with ingress 1/2/8 cells for the ingress-concurrency-sync study.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`4`

**How it was run:** config `scale-sync-ingress-4-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-8-forward-slow

Sync runtime with eight ingress workers under a slow stub upstream.

**Notes:** Optional noisy rung in the ingress series — keep in the catalog; publish may omit it if the lab is unstable.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`8`

**How it was run:** config `scale-sync-ingress-8-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-io-1-forward-slow

split_io under a slow stub upstream with one I/O worker and ingress/policy fixed at two each.

**Notes:** Part of the I/O-versus-ingress sizing series (io_workers 1/2/4/8).

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`1`

**How it was run:** config `scale-split-io-io-1-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-forward-slow

Throughput for the split_io runtime while every upstream answer is held for 50 ms (forward_slow).

**Notes:** Paired with scale-sync-forward-slow; slow upstream is where split_io should show its advantage.

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**How it was run:** config `scale-split-io-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-io-4-forward-slow

split_io under a slow stub upstream with four I/O workers and ingress/policy fixed at two each.

**Notes:** Part of the I/O-versus-ingress sizing series (io_workers 1/2/4/8).

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`4`

**How it was run:** config `scale-split-io-io-4-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-io-8-forward-slow

split_io under a slow stub upstream with eight I/O workers and ingress/policy fixed at two each.

**Notes:** Optional noisy rung in the I/O series — keep in the catalog; publish may omit it if the lab is unstable.

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`8`

**How it was run:** config `scale-split-io-io-8-obs-off.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-forward-fast

Throughput for the split_io runtime while forwarding to a fast stub upstream.

**Notes:** Thin published pair with scale-sync-forward-fast.

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `topology`=`thin`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**How it was run:** config `scale-split-io-obs-off.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-topology-heavy

Higher worker counts on split_io (ingress, policy, and I/O at four each) under a fast stub upstream — a threads-versus-cores topology check.

**Notes:** Runnable locally; not required in the published reference set.

**What varies:** `runtime`=[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), `load_shape`=[`forward_fast`](/performance/methodology.md#load-shapes), `topology`=`heavy`, `ingress_workers`=`4`, `policy_workers`=`4`, `io_workers`=`4`

**How it was run:** config `scale-split-io-topology-heavy.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-lmdb-cache-hit

Warm LMDB cache hits on the sync runtime using a real-disk environment path. After warm-up the load is read-mostly (near-100% hits).

**Notes:** Uses the fixed safe LMDB sync durability default. Not a high-churn fill/evict shape. Compare with scale-sync-cache-hit via the memory-vs-lmdb-cache-hit study.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`cache_hit`](/performance/methodology.md#load-shapes), `ingress_workers`=`2`, `cache_backend`=`lmdb`

**How it was run:** config `scale-sync-lmdb-cache-hit.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-8-memory-cache-churn

High-churn memory-cache load on sync with eight ingress workers: query set larger than max_entries, stub TTL long enough that entry-cap turnover drives misses, no warm plateau.

**Notes:** Primary memory member of study memory-vs-lmdb-cache-churn. Matched with scale-sync-ingress-8-lmdb-cache-churn (eight workers so the compare is not starved by a two-thread sync path). Recipe: 4096 unique names, max_entries 2048, stub TTL 60s. The thin-ingress pair (scale-sync-memory-cache-churn) remains as a companion cell, not the study primary.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`cache_churn`](/performance/methodology.md#load-shapes), `ingress_workers`=`8`, `cache_backend`=`memory`

**How it was run:** config `scale-sync-ingress-8-memory-cache-churn.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-8-lmdb-cache-churn

High-churn LMDB-cache load on sync with eight ingress workers and explicit multi-env sharding on a real-disk path. Matched entry cap, query diversity, and stub TTL keep continuous fill/evict pressure.

**Notes:** Primary LMDB member of study memory-vs-lmdb-cache-churn. Fixed safe sync durability; when_full=evict_one; shard_count=16 (2× ingress). Path /var/tmp/conduit-perf/lmdb-cache-churn-i8. Recipe matches the ingress-8 memory cell. Average latency under the elevated outstanding window still tracks roughly outstanding/QPS (Little's Law) — lead takeaways with relative QPS and hit/miss. Thin-ingress scale-sync-lmdb-cache-churn is a companion, not the study primary.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`cache_churn`](/performance/methodology.md#load-shapes), `ingress_workers`=`8`, `cache_backend`=`lmdb`

**How it was run:** config `scale-sync-ingress-8-lmdb-cache-churn.yml`; upstream `fast` (fast stub upstream); loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

## shutdown_drain

<div class="perf-scenario" markdown="1">

### shutdown-drain-complete-forward-slow

Stop under load with a long drain timeout budget (drain-complete) while forwarding to a slow stub upstream. Records drain duration and client loss during the stop window.

**Notes:** Paired with drain-budgeted and drain-minimal for relative comparison.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `drain_policy`=`drain_complete`, `ingress_workers`=`2`

**How it was run:** config `shutdown-drain-complete-forward-slow.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=4, threads=2 (dnsperf default outstanding ≈ 100).

</div>

<div class="perf-scenario" markdown="1">

### shutdown-drain-budgeted-forward-slow

Stop under load with a short drain timeout (drain-budgeted) while forwarding to a slow stub upstream. Records how long drain takes and how many client queries are lost in the stop window.

**Notes:** Paired with drain-complete and drain-minimal for relative comparison.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `drain_policy`=`drain_budgeted`, `ingress_workers`=`2`

**How it was run:** config `shutdown-drain-budgeted-forward-slow.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=4, threads=2 (dnsperf default outstanding ≈ 100).

</div>

<div class="perf-scenario" markdown="1">

### shutdown-drain-minimal-forward-slow

Stop under load with drain disabled (shutdown.drain false — no wait) while forwarding to a slow stub upstream. Records drain duration and client loss during the stop window.

**Notes:** Paired with drain-complete and drain-budgeted for relative comparison.

**What varies:** `runtime`=[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), `load_shape`=[`forward_slow`](/performance/methodology.md#load-shapes), `drain_policy`=`drain_minimal`, `ingress_workers`=`2`

**How it was run:** config `shutdown-drain-minimal-forward-slow.yml`; upstream `slow` (slow stub upstream (50 ms hold)); loadgen `dnsperf`; clients=4, threads=2 (dnsperf default outstanding ≈ 100).

</div>
<!-- perf-scenarios-body:end -->

## Related

- [Performance findings](/performance/index.md#findings)
- [Tuning evidence (studies)](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md)
