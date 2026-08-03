# Reproduce performance suites against a binary

Replay published suite kinds without a Rust toolchain for the system under test.
You need a Conduit **binary**, Python 3 harness dependencies, and Docker for the
default dnsperf path (native dnsperf is optional).

## Prerequisites

1. Obtain a Conduit binary (release asset, package, or a binary you already built).
2. Clone or unpack a checkout that includes the `perf/` tree.
3. Install harness Python dependencies:

    ```zsh
    pip install -r perf/requirements.txt
    ```

4. Docker available for the pinned dnsperf image (default). Build once if needed:

    ```zsh
    docker build -t dnsconduit-dnsperf:2.14.0 \
      -f perf/fixtures/dnsperf/Dockerfile \
      perf/fixtures/dnsperf
    ```

## List and run

```zsh
cd /path/to/DNSConduit
python3 -m perf.runner list --suite scale
python3 -m perf.runner list --curated

python3 -m perf.runner run \
  --conduit /path/to/conduit \
  --suite scale \
  --profile-id local \
  --render plain
```

Select by suite and/or scenario id (`--suite` and `--scenario` are repeatable).
You can also select by **study** (expands to member scenarios) or **publish-set**
(union of members from studies marked published):

```zsh
python3 -m perf.runner list --studies -v
python3 -m perf.runner list --study sync-vs-split-io
python3 -m perf.runner run \
  --conduit /path/to/conduit \
  --study metrics-scrape-ladder \
  --profile-id local \
  --time 5 \
  --render plain
# Lab refresh for curated studies (omit --time for publish-quality duration):
python3 -m perf.runner run \
  --conduit /path/to/conduit \
  --publish-set \
  --profile-id maintainer-ws-1 \
  -o perf/results/runs/publish-set-lab.json
```

`--time 5` (or similar) is fine for **smoke** development. Promoted reference
JSON for published studies SHOULD share a consistent **publish-quality** window
on the single reference host (`maintainer-ws-1`) — do not mix smoke cells into
published same-host comparisons without re-running the publish-set at that quality.

Loadgen concurrency defaults match published methodology (`--clients 4`,
`--dnsperf-threads 2`, no `--max-outstanding` → dnsperf default ≈ 100). For
elevated probes:

```zsh
python3 -m perf.runner run \
  --conduit /path/to/conduit \
  --scenario scale-split-io-forward-fast \
  --clients 16 \
  --dnsperf-threads 8 \
  --max-outstanding 2000 \
  --profile-id maintainer-ws-1 \
  --render plain
```

Pass `--loadgen-mode native` when `dnsperf` is on `$PATH` and you prefer not to use Docker.

Decision-shaped comparisons for operators live under
[Tuning evidence (studies)](/performance/studies/index.md).

## Re-render without re-running

```zsh
python3 -m perf.runner render \
  --from perf/results/runs/run-….json \
  --format rich
```

Formats: `plain`, `rich`, `yaml`, `json`, `html`.

## Annotations (harness footnotes)

Stable footnotes live under `perf/catalog/annotations/` (YAML catalog). Attach them
at run or scenario level when recording a promote; `make perf-docs` renders each
id as an include fragment under `operator-docs/docs/performance/includes/` and
injects marked pages (for example methodology / reference load-shape notes).

```zsh
python3 -m perf.runner annotations -v
python3 -m perf.runner run \
  --conduit /path/to/conduit \
  --scenario scale-sync-forward-fast \
  --annotation-id ann-example-context
```

## Maintainer publish

Operator-facing interpretation (load shapes, elevated vs thin recipes, median-of-3,
answer gate, CPU governor requirement) stays on
[Methodology](/performance/methodology.md). This section is the **publish pipeline**
for refreshing committed reference JSON and regenerating docs.

### Promote vs docs render

| Layer | Who | What |
|-------|-----|------|
| Measure + promote | Maintainer, on the single reference host | Run suites; land validated JSON under `perf/results/references/` via pull request |
| Docs representation | Docs / CI generate step | From **committed** JSON only: regenerate tables, static SVG, CSV — **no** live Conduit or dnsperf |

Stale references are fixed by a maintainer lab refresh and PR, not by an automated
bench on the release tag.

### Lab refresh (`--publish-set`)

On `maintainer-ws-1`, set CPU governors to `performance` when the host offers that
governor (via `cpupower`, `powerprofilesctl`, or sysfs — see `perf/README.md`, Host
CPU power state). Pass `--allow-suboptimal-cpu-power` only for an intentional noisy
probe; that override is not publish-quality.

Raise UDP socket memory so fixture `listeners.rcvbuf` (4 MiB) is not clamped by
`net.core.rmem_max` (see `perf/README.md`, Host UDP receive buffers):

```zsh
sudo sysctl -w net.core.rmem_max=16777216 net.core.rmem_default=4194304
```

The harness refuses elevated runs when `rmem_max` is below 4 MiB unless you pass
`--allow-suboptimal-udp-buffers` (expect Queries lost from kernel `RcvbufErrors`).

Omit short `--time` overrides for publish-quality duration. Curated publish uses
**median of 3 independent rounds** (not a single draw); see
[Methodology](/performance/methodology.md). Example:

```zsh
# N=3 publish-set → merge-median → ok-only JSON (then promote)
make perf-run-publish-set-median CONDUIT=/path/to/conduit \
  PERF_PROFILE_ID=maintainer-ws-1 \
  PERF_ROUNDS=3 \
  PERF_MEDIAN_DIR=perf/results/runs/publish-set-median

make perf-promote \
  PERF_FROM=perf/results/runs/publish-set-median/median-promotable.json \
  PERF_PROFILE_ID=maintainer-ws-1 \
  PERF_PROMOTE_MODE=publish-set
```

If one ranking cell's per-round range in `quality.notes` is large (roughly
≳20–25% of the median), remeasure that **subset** at `PERF_ROUNDS=5` rather than
raising the default bag-wide N. Promote the validated median into
`perf/results/references/`, then regenerate operator docs (`make perf-docs`).
Annotation catalog wiring is in
[Annotations](#annotations-harness-footnotes) above.

## Make targets

From a checkout: `make perf-list`, `make perf-run-scale CONDUIT=/path/to/conduit`,
`make perf-run-publish-set-median`, `make perf-render PERF_FROM=…` (optional
`PERF_FORMAT=plain|rich|yaml|json|html`; `FORMAT=` is an alias).
`make performance` remains the microbench path and is distinct from these load
suites.

## What you do not need

- `rustc` / `cargo` to **replay** suites against a prebuilt binary
- The OTLP lab receiver for scrape-only metrics scenarios (required only for OTLP
  `feature_tax` cells; release packages include `conduit-otlp-metrics-tracer` when
  companions are shipped)

See also [Methodology](/performance/methodology.md) and the in-tree `perf/README.md`.
