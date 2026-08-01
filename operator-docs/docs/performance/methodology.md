# Performance methodology

How published performance numbers are produced, what they mean, and how to interpret
charts and tables.

## Suites

| Suite | What it measures | Primary metrics |
|-------|------------------|-----------------|
| `scale` | Runtime models ([`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) / [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)) under paired load shapes | Achieved QPS, latency |
| `shutdown_drain` | [Drain policies](/concepts/runtime-and-concurrency.md#graceful-drain-on-shutdown) under load (especially slow upstream) | Drain duration, client loss during stop |
| `feature_tax` | Cost of [metrics](/observability/metrics-configurability.md) / [dnstap](/observability/event-export.md) / [OTLP](/guides/otlp-metrics-push.md) / logging / tracing relative to observability off (including collect-vs-emit, combined surfaces, [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) scrape, and scrape-hammer cadence) | QPS / latency delta vs baseline |
| `lifecycle` | Cold start and config apply | Cold-start ms, apply latency |
| `lossless_upgrade` | Zero-downtime upgrade handoff (when available) | Gap / loss, child READY, drain duration |

Maintainer Layer A microbenchmarks (`make performance`) are separate and are not the
published contract.

## Load shapes

Answer-source shapes used by forward suites:

| Shape | Meaning |
|-------|---------|
| `forward_fast` | Stub upstream answers quickly |
| `forward_slow` | Stub upstream with artificial delay (stresses outstanding work) |
| `cache_hit` | [Lookup cache](/guides/dns-answer-cache.md) enabled and warmed |

Runtime compares **must** present paired shapes — especially [`forward_slow`](/performance/methodology.md#load-shapes) for
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) — so charts do not favor only the shape that helps one model.
See [Sync vs split_io](/performance/studies/sync-vs-split-io.md).

<!-- perf-ann:ann-forward-slow-lossy-context:start -->
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
<!-- perf-ann:ann-forward-slow-lossy-context:end -->

## Drain policy vocabulary

Shared by `shutdown_drain` (and future lossless upgrade):

| Policy | Intent |
|--------|--------|
| `drain_complete` | Wait until idle (or a very long budget) before exit |
| `drain_budgeted` | Configured [`drain_timeout_ms`](/reference/config-schema/shutdown.md) budget |
| `drain_minimal` | Near-zero wait / `drain: false` cutover |

Config fields: [Shutdown](/reference/config-schema/shutdown.md). Comparative evidence:
[Drain policy under slow upstream](/performance/studies/drain-policy-under-slow.md).

## Lab profiles

Every run records a lab host profile (id, CPU, cores, OS, binary identity, loadgen).
The harness supports **many** profile ids. **v1 curated publish** promotes from **one**
named reference profile: `maintainer-ws-1` (maintainer workstation). Published charts
compare scenarios **on that same host**; they are not cross-host capacity claims.
See the disclaimer on the [performance hub](/performance/index.md).

The lab runner also refuses to start when CPU frequency governors are not all
`performance` **and** that governor is offered by the host (powersave /
schedutil / mixed governors introduce large QPS swings). Boards that never
expose `performance` are not blocked. Maintainers typically set performance
via `cpupower`, `powerprofilesctl`, or a direct sysfs write before a publish
refresh, or pass `--allow-suboptimal-cpu-power` for an intentional noisy
probe. See `perf/README.md` (Host CPU power state).

## Primary load generator

Published QPS / latency cells use **DNS-OARC dnsperf**. The default obtain-and-run path
is a **pinned Docker image** (`dnsconduit-dnsperf:2.14.0` from `perf/fixtures/dnsperf/`).
A native `dnsperf` on `$PATH` is an optional override. Exact flags and query files live
in `perf/README.md` in a source checkout.

### How load is applied (not a fixed offered QPS)

Published scale / feature-tax cells are **not** “sustain X queries per second”
runs. dnsperf sends as fast as its **outstanding-queries window** allows, and
that window's size is the single most important knob for whether a chart
measures Conduit's own capacity or just restates 1/latency:

```text
dnsperf … -c <clients> -T <threads> -l <seconds>
```

| Knob | Meaning |
|------|---------|
| `-l` | Timed window (CLI `--time`; publish-quality omits short smoke overrides) |
| `-c` / `-T` | Parallel client sockets and send/receive pairs |
| `-Q` | **Not set** for any published cell — no offered-QPS cap |
| `-q` | Max **outstanding** queries; sender pauses when this many are in flight until replies or timeouts free slots |

By Little's Law, achieved QPS × average latency ≈ outstanding queries. With a
**thin** window (dnsperf default `-q` ≈ 100, `-c 4`/`-T 2`), a fast round trip
(sub-millisecond) means QPS is almost entirely a restatement of
100 / latency — two configurations that both answer quickly can trade chart
rank on nothing but a fraction of a millisecond of host noise, even though
neither is actually CPU- or capacity-bound. That thin window is only a
trustworthy throughput signal when the round trip is *slow enough*, or
deliberately used as an artificial constraint, that the outstanding window
itself is the intended object of study.

**Recipe by load shape:**

| Load shape | Recipe | Why |
|------------|--------|-----|
| `forward_fast`, `cache_hit` (scale, feature_tax) | Elevated: `clients` 16 / `dnsperf_threads` 8 / `max_outstanding` 2000 | Round trip is fast enough that the thin window saturates on outstanding alone (see above). Elevating the window lets achieved QPS reflect Conduit's own processing capacity instead. |
| `forward_slow` (scale ladders, `shutdown_drain`, `lossless_upgrade`) | Thin: dnsperf default (`-q` ≈ 100), `-c 4`/`-T 2` | The artificial upstream delay is the variable under study (worker-ladder response to a slow backend, drain behavior under load) — elevating outstanding here would just add more queries behind the same delay, not change what's measured. |

Published `forward_fast`/`cache_hit` cells in `scale` and `feature_tax` are
also each the **median of 3 independent rounds** (same scenario, same
recipe, rerun end to end) rather than a single draw, so a one-off scheduling
hiccup on the shared lab host cannot flip a ranking. The observed per-round
range is recorded on each cell's `quality.notes` in the reference JSON.
`forward_slow`, `shutdown_drain`, and `lifecycle` cells remain single-shot.

Maintainer labs that raise `--max-outstanding` further (or lower it, to
reproduce the thin default) are a **separate** probe and are not
interchangeable with published cells unless the reference JSON and charts are
refreshed under that recipe.

Separate from dnsperf: Conduit fixtures also set
`forward.outstanding_per_backend` (and related caps). Those are **server-side**
in-flight limits, not the loadgen `-q` window.

For reproduce commands, see [Reproduce against a binary](/performance/reproduce.md).

## Promote vs docs render

| Layer | Who | What |
|-------|-----|------|
| Measure + promote | Maintainer, on the reference lab profile | Run suites; land validated JSON under `perf/results/references/` via pull request |
| Docs representation | Docs / CI generate step | From **committed** JSON only: regenerate tables, static SVG, CSV — **no** live Conduit or dnsperf |

Stale references are fixed by a maintainer lab refresh and PR, not by an automated
bench on the release tag.

## Studies (comparative evidence)

[Tuning evidence (studies)](/performance/studies/index.md) are catalog overlays that
select scenario cells by id for operator questions (runtime choice, metrics tax,
drain policy, and similar). Studies are **not** a separate measurement driver —
`--study` / `--publish-set` expand to scenario members. Generated study pages embed
static SVG/CSV from the same promoted JSON as the reference warehouse. Prefer studies
for decision narrative; use [reference results](/performance/reference.md) for the
dense chart warehouse.

## Charts, CSV, and tables

Operator-docs charts are **static SVG** derived from promoted JSON. Paired load shapes
that differ by orders of magnitude (for example `forward_fast` vs `forward_slow`) use
**separate charts** so each Y-axis stays readable. Each chart has a **CSV download** of
the values shown in that figure. Tables include richer columns (sent / completed /
lost, workers, latency) when recorded, and scenario labels link to
[scenario descriptions](/performance/scenarios.md). Missing series are omitted or
marked unavailable — numbers are never fabricated.

Harness maintainers may attach stable footnotes from `perf/catalog/annotations/`
at promote time (run- or scenario-level). Published pages pull those notes in as
include fragments next to the load shapes or studies they qualify — there is no
separate annotations index page.
