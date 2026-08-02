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
| `forward_fast` | Stub upstream answers immediately |
| `forward_slow` | Stub upstream holds every answer for a fixed 50 ms |
| `cache_hit` | [Lookup cache](/guides/dns-answer-cache.md) enabled and warmed |

### The stub upstream is never the constraint

Forward shapes answer from a stub responder, not a real resolver, and the stub is
built so that it cannot be what a chart is measuring:

- Replies are served by **several worker processes sharing one port**, so the
  responder scales past a single CPU. Loaded directly, with no Conduit in the
  path, it answers roughly 480k QPS on the fast shape — about three times the
  highest rate any published cell drives through it.
- The delayed responder **queues** its replies rather than sleeping on them, so
  `forward_slow` models a backend that is slow but still accepts new queries —
  the delay never serializes into one answer per 50 ms. On that shape the
  harness tops out near 39k QPS, which is the load generator's own in-flight
  window rather than the responder; the responder loses no queries there.
- On a lab host with mixed fast and efficient CPU cores, Conduit runs on the
  fast cores and **every harness process** — load generator, stub responder,
  and telemetry receivers — is confined to the others, so the measurement
  apparatus cannot take CPU away from the thing being measured.

A stub that saturates does not simply cap a chart; it changes what the chart
measures, because Conduit then starts refusing queries it cannot forward. See
[Only successful answers count](#only-successful-answers-count) for how the
harness detects that.

Runtime compares **must** present paired shapes — especially [`forward_slow`](/performance/methodology.md#load-shapes) for
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) vs
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) — so charts do not favor only the shape that helps one model.
See [Sync vs split_io](/performance/studies/sync-vs-split-io.md).

<!-- perf-ann:ann-forward-slow-lossy-context:start -->
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
| `forward_fast`, `cache_hit`, `forward_slow` (scale, feature_tax) | Elevated: `clients` 16 / `dnsperf_threads` 8 / `max_outstanding` 2000 | One recipe across published comparative cells. The elevated window lets achieved QPS reflect Conduit's own capacity instead of restating 1/latency, and on the slow shape it keeps a multiplexing runtime from being capped by the loadgen rather than by Conduit. |
| `shutdown_drain`, `lossless_upgrade` | Thin: dnsperf default (`-q` ≈ 100), `-c 4`/`-T 2` | These cells measure drain timing and client loss during a stop window, not steady-state throughput. |

`forward_slow` cells run a **30 second** window instead of 10. A runtime that
blocks on a 50 ms upstream completes very few queries, and a short window would
leave too small a sample to mean anything.

Published `scale` and `feature_tax` cells are each the **median of 3
independent rounds** (same scenario, same recipe, rerun end to end) rather than
a single draw, so a one-off scheduling hiccup on the shared lab host cannot flip
a ranking. The observed per-round range is recorded on each cell's
`quality.notes` in the reference JSON. `shutdown_drain` and `lifecycle` cells
remain single-shot.

### Only successful answers count

A forwarding proxy that cannot forward still answers: it returns SERVFAIL, and
it does so in microseconds. A load generator counts those refusals as completed
queries, so an overwhelmed configuration can report a *higher* QPS and a *lower*
average latency than a healthy one. Published throughput must never be built on
that.

Every load-bearing cell therefore records the loadgen's response-code
breakdown, and the harness compares it against the answer the scenario is meant
to produce (`NOERROR` for forward and cache shapes):

- At least **99%** successful answers: the cell is a valid measurement.
- Below that: the cell is recorded as **invalid**, and promoting a reference
  that contains it fails outright. There is no override for published data.

Cells where failed answers are the subject of the measurement — the
`shutdown_drain` stop window, for example — opt out of the check explicitly in
the scenario catalog.

Both parts of the check appear in the reference JSON as
`metrics.response_codes` and `metrics.answer_ok_percent`.

Maintainer labs that raise `--max-outstanding` further (or lower it, to
reproduce the thin default) are a **separate** probe and are not
interchangeable with published cells unless the reference JSON and charts are
refreshed under that recipe.

Separate from dnsperf: Conduit fixtures also set
`forward.outstanding_per_backend` and `orchestrator.txn_table_capacity`. Those
are **server-side** in-flight limits, not the loadgen `-q` window. Lab fixtures
size them well above the offered window (8192 and 65536) so that a published
chart measures the runtime rather than a fixture ceiling; production values
should be chosen for your own backends, not copied from the lab.

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
