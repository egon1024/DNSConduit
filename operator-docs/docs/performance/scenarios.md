---
toc_depth: 3
toc_collapsible: true
---

# Performance scenarios

Deep-link glossary for each curated performance scenario — what it measures and
which axes it holds. Table rows on
[reference results](/performance/reference.md) and study evidence link here.
This page is **not** the primary decision surface; start from
[Performance findings](/performance/index.md#findings) or
[Tuning evidence (studies)](/performance/studies/index.md).

Axes such as [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) /
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

Dnstap/events off (no sinks) under forward_fast — pair baseline for sampled/fuller.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`dnstap_off`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-dnstap-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-dnstap-sampled-forward-fast

Dnstap sampled (~10 percent response frames) under forward_fast with lab tracer.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`dnstap_sampled`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-dnstap-sampled.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-dnstap-full-forward-fast

Dnstap fuller emit (query, response, retry) under forward_fast with lab tracer.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`dnstap_full`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-dnstap-full.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-logging-warn-forward-fast

Metrics off with logging.level warn under forward_fast — verbosity tax warn
pole paired with feature-tax-logging-debug-forward-fast. Named for the logging
axis (not the shared metrics_off baseline used by other feature_tax studies).

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`logging_warn`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-logging-warn.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-logging-debug-forward-fast

Metrics off with logging.level debug under forward_fast — verbosity tax pair
with feature-tax-logging-warn-forward-fast.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`logging_debug`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-logging-debug.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-no-collect-forward-fast

No-collect on hot-path categories under forward_fast — neither record nor export
for volume/failures/lookup/timing. Paired with collect-only and collect+emit.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_no_collect`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-no-collect.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-collect-only-forward-fast

Collect hot-path categories with emit false under forward_fast — records without
scrape/OTLP export. Paired with collect+emit and no-collect.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_collect_only`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-collect-only.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-collect-emit-forward-fast

Collect+emit on standard base under forward_fast — paired with collect-only and
no-collect for emit-path secondary comparison.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_collect_emit`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-collect-emit.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-off-forward-fast

Observability-off baseline under forward_fast — metrics disabled, no dnstap sinks.
Baseline pair for minimal/standard scrape tax deltas.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_off`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-forward-fast

Metrics base standard with Prometheus scrape (collect+emit) under forward_fast.
Scrape-only high pole; also the collect+emit pole of the collect pair.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_standard_scrape`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-standard-scrape.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-dnstap-full-forward-fast

Combined tax — metrics standard scrape plus fuller dnstap under forward_fast.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_standard_dnstap_full`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-standard-dnstap-full.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-hammer-forward-fast

Standard scrape with a lab scrape hammer (~100ms GET loop) under forward_fast —
aggressive scrape cadence vs listener-only standard scrape.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_standard_scrape_hammer`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-standard-scrape.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-off-scrape-ladder-forward-fast

Observability-off baseline for the metrics scrape tax under an elevated
dnsperf outstanding window (publish recipe for this study). Pair with
minimal/standard scrape-tax cells — not the shared metrics-off cell used
by other feature_tax studies.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_off`, `ingress_workers`=`2`, `loadgen_recipe`=`elevated_outstanding`

**Recipe:** config `feature-tax-metrics-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-minimal-scrape-ladder-forward-fast

Metrics base minimal + Prometheus scrape under elevated dnsperf outstanding
(scrape-tax publish recipe). Pair with off/standard scrape-tax cells.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_minimal_scrape`, `ingress_workers`=`2`, `loadgen_recipe`=`elevated_outstanding`

**Recipe:** config `feature-tax-metrics-minimal-scrape.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-ladder-forward-fast

Metrics base standard + Prometheus scrape under elevated dnsperf outstanding
(scrape-tax publish recipe). Pair with off/minimal scrape-tax cells.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_standard_scrape`, `ingress_workers`=`2`, `loadgen_recipe`=`elevated_outstanding`

**Recipe:** config `feature-tax-metrics-standard-scrape.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-off-split-io-forward-fast

Observability-off baseline under split_io + forward_fast — pair for scrape tax
on the split_io runtime model.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_off`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**Recipe:** config `feature-tax-metrics-off-split-io.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-standard-scrape-split-io-forward-fast

Metrics base standard with Prometheus scrape under split_io + forward_fast —
scrape tax pair with feature-tax-metrics-off-split-io-forward-fast.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_standard_scrape`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**Recipe:** config `feature-tax-metrics-standard-scrape-split-io.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-otlp-push-forward-fast

OTLP HTTP metrics push under forward_fast. Requires conduit-otlp-metrics-tracer;
records secondary otlp_accepts / otlp_failures from the lab receiver.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_otlp_push`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-otlp-push.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-tracing-on-forward-fast

Metrics off with pipeline tracing enabled (qtype A @ 100% sample) under
forward_fast — pair with feature-tax-metrics-off-forward-fast.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`tracing_on`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-tracing-on.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### feature-tax-metrics-minimal-scrape-forward-fast

Metrics base minimal with Prometheus scrape (collect+emit) under forward_fast.
Middle cell between metrics-off and standard scrape.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_minimal_scrape`, `ingress_workers`=`2`

**Recipe:** config `feature-tax-metrics-minimal-scrape.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

## lifecycle

<div class="perf-scenario" markdown="1">

### lifecycle-cold-start

Cold-start wall time from process start to first successful DNS answer.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_off`, `ingress_workers`=`2`

**Recipe:** config `lifecycle-cold-start.yml`; upstream `fast`; loadgen `none`.

</div>

<div class="perf-scenario" markdown="1">

### lifecycle-config-apply

Config apply latency via conduitctl apply sparse overlay (logging level).
Thin lifecycle cell; not required for curated publish spine.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `obs_posture`=`metrics_off`, `ingress_workers`=`2`

**Recipe:** config `lifecycle-config-apply-base.yml`; upstream `fast`; loadgen `none`.

</div>

## scale

<div class="perf-scenario" markdown="1">

### scale-sync-forward-fast

Compare sync runtime under a fast stub upstream (forward_fast load shape).
Thin curated spine pair with scale-split-io-forward-fast.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `ingress_workers`=`2`

**Recipe:** config `scale-sync-obs-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-cache-hit

Maintainer-runnable cache_hit load shape: sync runtime with memory cache warmed
before dnsperf. Not required for the initial curated publish spine.

**Axes:** `runtime`=`sync`, `load_shape`=`cache_hit`, `ingress_workers`=`2`

**Recipe:** config `scale-sync-cache-hit.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-1-forward-fast

Ingress concurrency (sync): one ingress worker under forward_fast.
Published forward_fast recipe (elevated outstanding) so achieved QPS reflects
Conduit ingress capacity rather than the loadgen outstanding window.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `ingress_workers`=`1`

**Recipe:** config `scale-sync-ingress-1-obs-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-4-forward-fast

Ingress concurrency (sync): four ingress workers under forward_fast.
Published forward_fast recipe (elevated outstanding) so achieved QPS reflects
Conduit ingress capacity rather than the loadgen outstanding window.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `ingress_workers`=`4`

**Recipe:** config `scale-sync-ingress-4-obs-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-8-forward-fast

Ingress concurrency (sync): eight ingress workers under forward_fast.
Published forward_fast recipe (elevated outstanding) so achieved QPS reflects
Conduit ingress capacity rather than the loadgen outstanding window.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_fast`, `ingress_workers`=`8`

**Recipe:** config `scale-sync-ingress-8-obs-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-1-forward-slow

Ingress concurrency (sync): one ingress worker under forward_slow.
Paired with ingress 2/4/8 cells for the ingress-concurrency-sync study.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `ingress_workers`=`1`

**Recipe:** config `scale-sync-ingress-1-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-forward-slow

Compare sync runtime under an artificially slow upstream (forward_slow).
Paired with scale-split-io-forward-slow for relative comparison.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `ingress_workers`=`2`

**Recipe:** config `scale-sync-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-4-forward-slow

Ingress concurrency (sync): four ingress workers under forward_slow.
Paired with ingress 1/2/8 cells for the ingress-concurrency-sync study.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `ingress_workers`=`4`

**Recipe:** config `scale-sync-ingress-4-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-sync-ingress-8-forward-slow

Ingress concurrency (sync): eight ingress workers under forward_slow.
Optional noisy rung — keep in catalog; publish may omit if unstable on the lab.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `ingress_workers`=`8`

**Recipe:** config `scale-sync-ingress-8-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-io-1-forward-slow

I/O vs ingress (split_io): one I/O worker with fixed ingress/policy=2
under forward_slow. Paired with io_workers 2/4/8 for io-vs-ingress-split.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_slow`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`1`

**Recipe:** config `scale-split-io-io-1-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-forward-slow

Compare split_io runtime under an artificially slow upstream (forward_slow).
Paired with scale-sync-forward-slow; slow upstream is where split_io should shine.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_slow`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**Recipe:** config `scale-split-io-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-io-4-forward-slow

I/O vs ingress (split_io): four I/O workers with fixed ingress/policy=2
under forward_slow. Paired with io_workers 1/2/8 for io-vs-ingress-split.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_slow`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`4`

**Recipe:** config `scale-split-io-io-4-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-io-8-forward-slow

I/O vs ingress (split_io): eight I/O workers with fixed ingress/policy=2
under forward_slow. Optional noisy rung — keep in catalog; publish may omit if unstable.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_slow`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`8`

**Recipe:** config `scale-split-io-io-8-obs-off.yml`; upstream `slow`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-forward-fast

Compare split_io runtime under a fast stub upstream (forward_fast load shape).
Thin curated spine pair with scale-sync-forward-fast.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_fast`, `topology`=`thin`, `ingress_workers`=`2`, `policy_workers`=`2`, `io_workers`=`2`

**Recipe:** config `scale-split-io-obs-off.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

<div class="perf-scenario" markdown="1">

### scale-split-io-topology-heavy

Maintainer topology / threads-vs-cores scenario: higher ingress, policy, and
I/O worker counts under forward_fast. Runnable locally; not required for G1
curated publish.

**Axes:** `runtime`=`split_io`, `load_shape`=`forward_fast`, `topology`=`heavy`, `ingress_workers`=`4`, `policy_workers`=`4`, `io_workers`=`4`

**Recipe:** config `scale-split-io-topology-heavy.yml`; upstream `fast`; loadgen `dnsperf`; clients=16, threads=8, max_outstanding=2000.

</div>

## shutdown_drain

<div class="perf-scenario" markdown="1">

### shutdown-drain-complete-forward-slow

Drain-complete policy (long drain_timeout_ms budget) under forward_slow load.
Records drain duration and client loss during the stop window. Paired with
budgeted and minimal policies for relative comparison.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `drain_policy`=`drain_complete`, `ingress_workers`=`2`

**Recipe:** config `shutdown-drain-complete-forward-slow.yml`; upstream `slow`; loadgen `dnsperf`; clients=4, threads=2 (dnsperf default outstanding ≈ 100).

</div>

<div class="perf-scenario" markdown="1">

### shutdown-drain-budgeted-forward-slow

Drain-budgeted policy (short drain_timeout_ms) under forward_slow load.
Records drain duration and client loss during the stop window. Paired with
complete and minimal policies for relative comparison.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `drain_policy`=`drain_budgeted`, `ingress_workers`=`2`

**Recipe:** config `shutdown-drain-budgeted-forward-slow.yml`; upstream `slow`; loadgen `dnsperf`; clients=4, threads=2 (dnsperf default outstanding ≈ 100).

</div>

<div class="perf-scenario" markdown="1">

### shutdown-drain-minimal-forward-slow

Drain-minimal policy (shutdown.drain false — no wait) under forward_slow load.
Records drain duration and client loss during the stop window. Paired with
complete and budgeted policies for relative comparison.

**Axes:** `runtime`=`sync`, `load_shape`=`forward_slow`, `drain_policy`=`drain_minimal`, `ingress_workers`=`2`

**Recipe:** config `shutdown-drain-minimal-forward-slow.yml`; upstream `slow`; loadgen `dnsperf`; clients=4, threads=2 (dnsperf default outstanding ≈ 100).

</div>
<!-- perf-scenarios-body:end -->

## Related

- [Performance findings](/performance/index.md#findings)
- [Tuning evidence (studies)](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md)
