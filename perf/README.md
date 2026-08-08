# DNSConduit performance benchmarking harness

Binary-driven performance suites: scenario catalog, canonical run JSON, multi-format
render, and curated operator-docs publish. Sibling to the interop correctness harness —
same *publishing pattern*, different drivers and success metrics.

**Not** a QPS SLO gate. Numbers are directional for a named lab host profile.
**Not** run as a full load suite in GitHub Actions on every PR.

## Quick start (binary replay — no rustc)

```zsh
cd ~/git_repos/DNSConduit
pip install -r perf/requirements.txt

# Build or obtain a Conduit binary however you prefer, then:
python3 -m perf.runner list --suite scale
python3 -m perf.runner list --suite shutdown_drain
python3 -m perf.runner run \
  --conduit ./target/release/conduit \
  --suite scale \
  --profile-id local \
  --render plain
```

Shutdown drain suite (SIGTERM under concurrent load):

```zsh
python3 -m perf.runner run \
  --conduit ./target/release/conduit \
  --suite shutdown_drain \
  --profile-id local \
  --time 5 \
  --render plain
```

Default loadgen is **DNS-OARC dnsperf** via a **pinned Docker image** (host networking
so the container reaches lab listeners on **127.0.2.1**). Native `dnsperf` on `$PATH`
is an override:

```zsh
python3 -m perf.runner run \
  --conduit ./target/release/conduit \
  --scenario scale-sync-forward-fast \
  --loadgen-mode native
```

## Host CPU power state

Before any scenario runs, the harness checks cpufreq `scaling_governor` on
every CPU that exposes it. When the host **offers** a `performance` governor
(`scaling_available_governors`), every CPU must already be in `performance`.
A `powersave` / `ondemand` / `schedutil` / mixed governor can swing measured
QPS by multiple× on the same binary — noise that dwarfs feature-tax deltas.

Boards that do **not** list `performance` among available governors (some ARM
/ embedded images with only `ondemand`/`schedutil`) are **not** blocked —
there is no better governor to require. Hosts without cpufreq sysfs (some
VMs/containers) also skip the check.

If the check fails, `run` exits **before** starting Conduit or dnsperf and
prints alternate remediation paths (whichever tools exist are marked on
`$PATH`; all are listed):

```zsh
# 1. cpupower (linux-tools / cpupower package):
sudo cpupower frequency-set -g performance
sudo cpupower frequency-set -g powersave   # restore example

# 2. power-profiles-daemon:
powerprofilesctl set performance
powerprofilesctl set balanced              # typical restore

# 3. Direct sysfs (no extra package):
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor >/dev/null
echo powersave   | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor >/dev/null

# Verify:
cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort -u

# 4. Override (results may be noisy):
python3 -m perf.runner run … --allow-suboptimal-cpu-power
```

Hybrid P-core/E-core pinning (when `/sys/devices/cpu_core` exists) is
separate and automatic — see `perf/runner/cpuaffinity.py`. Conduit gets the
P-cores; **everything else the harness runs** — dnsperf, the stub upstream
pool, and the dnstap / OTLP receivers — is confined to the E-cores. An
unpinned stub is not a neutral bystander: at a few hundred thousand QPS it is
several cores of work, and left free to land on P-cores it competes with the
process under test and swings identical rounds by 1.3–2.2×.

## Host UDP receive buffers

Elevated dnsperf recipes (clients 16 / threads 8 / `-q` 2000) can overflow a
tiny OS UDP receive buffer. Datagrams dropped by the kernel never reach
Conduit; dnsperf reports them as **Queries lost**, while Conduit counters show
`queries == responses` for every packet that arrived. On maintainer-ws-1 this
matched **kernel `RcvbufErrors` Δ == dnsperf Lost** exactly — not soft-stop
cutoff and not Conduit failing to answer received queries.

Perf fixtures set `listeners.rcvbuf: 4194304` (4 MiB). Linux clamps
`SO_RCVBUF` to `net.core.rmem_max`, so publish hosts must raise that sysctl
(and typically `rmem_default`) **before** suite runs. The harness refuses to
`run` when `rmem_max < 4 MiB` unless you pass
`--allow-suboptimal-udp-buffers`.

```zsh
# Session (or persist under /etc/sysctl.d/):
sudo sysctl -w net.core.rmem_max=16777216 net.core.rmem_default=4194304

# Verify:
sysctl net.core.rmem_max net.core.rmem_default

# Noisy probe only:
python3 -m perf.runner run … --allow-suboptimal-udp-buffers
```

See also operator-docs Performance methodology (incomplete answers / UDP
buffers) and Dataplane runtime tuning (`listeners.rcvbuf`).

Re-render without re-running:

```zsh
python3 -m perf.runner render --from perf/results/runs/run-….json --format rich
python3 -m perf.runner render --from perf/results/runs/run-….json --format html -o /tmp/perf.html
```

## Primary loadgen (dnsperf)

| Aspect | Value |
|--------|-------|
| Tool | DNS-OARC [dnsperf](https://github.com/DNS-OARC/dnsperf) |
| Default | Docker image `dnsconduit-dnsperf:2.14.0` built from `perf/fixtures/dnsperf/Dockerfile` (upstream **2.14.0**) |
| Network | `--network=host` so dnsperf reaches **127.0.2.1:15353** |
| Query file | `perf/fixtures/queries/perf-a.txt` |
| Offered QPS (`-Q`) | **Not set** for any published cell — open-as-fast-as-outstanding-allows |
| Override | `--loadgen-mode native` when `dnsperf` is on `$PATH` |

**Concurrency is recipe-driven, not one global default.** Scenario YAML sets
`clients` / `dnsperf_threads` / `max_outstanding` on the recipe when a cell
needs something other than the CLI defaults (`-c 4 -T 2`, dnsperf's own
`-q` ≈ 100 default):

| Suite / shape | Recipe baked into the scenario | Rationale |
|---------------|-------------------------------|-----------|
| `scale` and `feature_tax` comparative cells (`forward_fast`, `cache_hit`, `forward_slow`) | `clients: 16`, `dnsperf_threads: 8`, `max_outstanding: 2000` | Fast round trips make the thin default's achieved QPS ≈ 100 / latency (Little's Law) — a throughput chart that's actually just inverse-latency. On the slow shape the thin window caps a multiplexing runtime at 100 / 50 ms ≈ 2000 QPS, which measures the loadgen rather than Conduit. |
| `shutdown_drain`, `lossless_upgrade` | Thin CLI default (`-c 4 -T 2`, `-q` ≈ 100) | These measure drain timing and client loss during a stop window, not steady-state throughput. |

`forward_slow` scale cells also set `duration_s: 30`. A runtime that blocks on
a 50 ms upstream completes only a few hundred queries in the default 10 second
window, which is too small a sample to publish. An explicit `--time` still wins
so smoke runs stay fast.

CLI `--clients` / `--dnsperf-threads` / `--max-outstanding` still work for ad
hoc probes, but recipe values on curated scenarios win by default (see
`_effective_loadgen_knobs` in `perf/runner/execute.py`). Full rationale:
`operator-docs/docs/performance/methodology.md` (section **How load is
applied**).

Publish-quality `scale` and `feature_tax` cells are run for **3 independent
rounds** and merged with `python3 -m perf.runner merge-median` (per-scenario
field median; see **Median merge for multi-round publish** below) before
promotion — a single noisy round can no longer swing a published ranking.

## Stub upstream and the answer gate

Forward shapes answer from `perf/runner/upstream.py`, a pool of forked UDP
responders sharing one `SO_REUSEPORT` port (8 workers fast, 4 slow), confined
to the harness core class. The slow responder holds replies in a timer heap
rather than sleeping, so a 50 ms backend stays concurrent instead of
serializing one answer per delay.

Loaded directly with no Conduit in the path, the fast pool sustains ~480k QPS —
roughly triple the highest rate any published cell drives through it. The slow
pool tops out at ~39k QPS, but that number is dnsperf's outstanding window
(2000 in flight ÷ 50 ms), not the responder: it drops zero packets there, and
raising the worker count does not move it. Both ceilings sit far above what
published cells ask for.

This matters because a saturated stub does not merely cap a chart. Conduit
starts refusing queries it cannot forward, and SERVFAIL in microseconds reads
to dnsperf as a completed query — so the contaminated cell reports *higher* QPS
and *lower* latency than a healthy one.

Every dnsperf cell therefore records the response-code histogram
(`metrics.response_codes`) and the share matching the answer the scenario
expects (`metrics.answer_ok_percent`, default `NOERROR`). Below **99%** the
scenario is recorded with `status: invalid`, and `perf.runner promote` refuses
the whole document rather than publishing it. Scenarios where failed answers
are the point opt out with `min_answer_ok_percent: 0` (the drain cells do).

Lab fixtures also size `forward.outstanding_per_backend` (8192) and
`orchestrator.txn_table_capacity` (65536) above the offered window, so a
published chart measures the runtime rather than a fixture ceiling.

Build the image once:

```zsh
docker build -t dnsconduit-dnsperf:2.14.0 \
  -f perf/fixtures/dnsperf/Dockerfile \
  perf/fixtures/dnsperf
```

The harness will attempt this build on first Docker run if the image is missing.

## Layout

| Path | Purpose |
|------|---------|
| `catalog/scenarios/` | YAML scenarios (id, suite, intent, axes, recipe) |
| `catalog/studies/` | Comparative studies (question, members, figures) |
| `catalog/lab_profiles/` | Named host profiles (template + filled instances) |
| `catalog/annotations/` | Stable-id footnotes (tone, title, body) |
| `runner/` | Python CLI (`python3 -m perf.runner`) |
| `fixtures/` | Conduit configs, query files, dnsperf Dockerfile, upstream recipes |
| `helpers/` | Pointers to companion Rust lab binaries |
| `results/schema.json` | Canonical run document schema |
| `results/runs/` | Append-oriented run JSON |
| `results/references/` | Curated promoted snapshots (manual PR) |
| `render/` | plain / rich / yaml / json / html from run JSON |

## Suites

| Suite | Focus |
|-------|-------|
| `scale` | Runtime models × load shapes (`forward_fast`, `forward_slow`, `cache_hit`) |
| `shutdown_drain` | Three drain policies (`drain_complete` / `drain_budgeted` / `drain_minimal`) under `forward_slow` load; records `drain_duration_ms` and `client_failures_during_stop` |
| `feature_tax` | Metrics scrape tax (off / minimal / standard scrape), collect vs emit, dnstap off/sampled/fuller, combined metrics+dnstap, OTLP push, logging/tracing tax, split_io scrape, scrape-hammer cadence |
| `lifecycle` | Cold start to first answer; thin config apply via `conduitctl` |
| `lossless_upgrade` | Gated on zero-downtime upgrade — skipped until available |

## Lab ports

Matches the maintainer lab map: Conduit DNS **127.0.2.1:15353**, stub upstream
**127.0.2.1:15300**, control **127.0.2.1:5199**, Prometheus scrape **127.0.2.1:19090**,
dnstap socket **unix:/tmp/conduit-perf-dnstap.sock**, OTLP HTTP
**http://127.0.2.1:4318/v1/metrics**.

## Companion: OTLP metrics tracer

OTLP `feature_tax` scenarios start **`conduit-otlp-metrics-tracer`** automatically when
the binary is next to `--conduit` (or passed via `--otlp-tracer`). Scrape-only scenarios
do not require it. Build from a source checkout:

```zsh
cargo build -p conduit-otlp-metrics-tracer --release
```

Release tarballs and packages include the prebuilt companion alongside
`conduit-dnstap-tracer`. See `perf/helpers/README.md`.

## Make targets

```zsh
make perf-unit          # harness unit tests (no live loadgen)
make perf-ui            # optional Textual lifecycle TUI (see below)
make perf-list          # list catalog
make perf-run-scale     # run scale suite (requires CONDUIT=)
make perf-run-shutdown-drain  # run shutdown_drain suite
make perf-run-feature-tax     # run feature_tax suite
make perf-run-lifecycle       # run lifecycle suite
make perf-run-study PERF_STUDY=sync-vs-split-io PERF_TIME=5   # smoke one study
make perf-run-publish-set     # union of published study members (lab refresh)
make perf-render        # render PERF_FROM=… PERF_FORMAT=plain|rich|yaml|json|html
                        # (FORMAT= is accepted as an alias for PERF_FORMAT=)
make perf-promote       # promote PERF_FROM run JSON into results/references/
make perf-docs          # SVG/CSV/tables + takeaway integrity check (no load suite)
```

`PERF_TIME=5` (or any short window) is for **smoke** development runs. Promoted
reference JSON for published studies SHOULD use the harness **default** duration
on the reference profile (`maintainer-ws-1`) — omit `PERF_TIME` for publish-quality.
`make performance` remains the **microbench** (Rhai Criterion) path and is distinct.
`make docs-build` runs `perf-docs` first so operator-docs stay aligned with committed JSON.

### Optional lifecycle TUI

A Textual UI wraps the same facade the CLI uses (run → merge/promote → generate-docs)
with sync badges between stages. It is optional and not required for docs CI:

```zsh
pip install -r perf/requirements-tui.txt
make perf-ui
# or: PYTHONPATH=. python3 -m perf.tui
```

### Takeaway integrity (Gate G5)

`make perf-docs` / `python3 -m perf.runner generate-docs` also:

1. Writes an operator-facing **At a glance** summary
   (`generated/study-<id>-deltas.fragment.md`) and injects it above each study
   **Takeaway** (`<!-- perf-study-deltas:… -->`).
2. Fails if Takeaway `×` / `%` / `~Nk` / `ms` claims disagree with Evidence poles
   beyond rounding (±1k QPS, one-decimal ×, ±1%, ±1 ms).
3. Fails on a small **banned-phrase** list (stale “same-host noise” / inversion
   hedges) defined in `perf/runner/integrity.py` (`BANNED_TAKEAWAY_PHRASES`).

Docs CI runs this via `make docs-build`. Unit coverage: `make perf-unit`
(`perf/runner/test_integrity.py`), including a deliberately stale takeaway case.

## Annotations

Stable footnotes live under `catalog/annotations/` (one YAML file per id). List them:

```zsh
python3 -m perf.runner annotations -v
```

Attach at run level and/or per scenario:

```zsh
python3 -m perf.runner run \
  --conduit ./target/release/conduit \
  --scenario scale-sync-forward-fast \
  --annotation-id ann-example-context \
  --scenario-annotation scale-sync-forward-fast=ann-thin-spine-context
```

`make perf-docs` writes each catalog id to
`operator-docs/docs/performance/includes/<id>.fragment.md` and injects pages that
declare `<!-- perf-ann:<id>:start -->` … `<!-- perf-ann:<id>:end -->` markers
(there is no separate Annotations nav page). Author release-tied notes in git;
prefer `related_releases` when explaining a published delta.

## Studies (comparative subsets)

Studies under `catalog/studies/` select scenario cells by id for operator-facing
comparisons. They are not a separate measurement driver.

```zsh
python3 -m perf.runner list --studies -v
python3 -m perf.runner list --study sync-vs-split-io
python3 -m perf.runner run \
  --conduit ./target/release/conduit \
  --study metrics-scrape-ladder \
  --profile-id local \
  --time 5 \
  --render plain
# Union of all published study members (lab refresh / publish path):
python3 -m perf.runner run \
  --conduit ./target/release/conduit \
  --publish-set \
  --profile-id maintainer-ws-1 \
  -o perf/results/runs/publish-set.json
```

Smoke durations (`--time 5` / `PERF_TIME=5`) are fine for development. Promoted
reference JSON for published studies SHOULD use a consistent publish-quality
window on the reference lab profile (omit `--time` / `PERF_TIME`).

## Median merge for multi-round publish

**Default publish bar: N=3** (median, not mean). Run the same selection three
times, merge with `merge-median`, strip answer-gate invalids, then promote.
That applies to curated **publish-set** refreshes and to `scale` /
`feature_tax` comparative cells (including `forward_fast`, `cache_hit`, and
`forward_slow` members in the bag). Smoke (`PERF_TIME=5`) and ad-hoc probes
stay single-shot. `lifecycle` outside the publish-set path stays single-shot.

**Selective N=5:** if a ranking cell's per-round min–max span in
`quality.notes` is large relative to the median (roughly ≳20–25%), remeasure
**that subset** (study or scenario list) at N=5 and merge-median again. Do not
raise the default bag-wide N to 5.

Canonical publish-set campaign (or `make perf-run-publish-set-median`):

```zsh
OUTDIR=perf/results/runs/publish-set-median
mkdir -p "$OUTDIR"
for r in 1 2 3; do
  PYTHONPATH=. python3 -m perf.runner run \
    --conduit ./target/release/conduit \
    --publish-set \
    --profile-id maintainer-ws-1 \
    --kill-strays \
    -o "$OUTDIR/r$r.json"
done

PYTHONPATH=. python3 -m perf.runner merge-median \
  --from "$OUTDIR/r1.json" \
  --from "$OUTDIR/r2.json" \
  --from "$OUTDIR/r3.json" \
  -o "$OUTDIR/median.json"
```

Per scenario, numeric `metrics`/`secondary` fields become the median across
rounds with `status: ok`; the observed range is recorded in
`quality.notes`. Non-numeric fields (axes, intent) come from the last round.

Suite-only example (same N=3 rule):

```zsh
PYTHONPATH=. python3 -m perf.runner run --conduit ./target/release/conduit \
  --suite feature_tax --profile-id maintainer-ws-1 \
  -o perf/results/runs/feature-tax-r1.json
# … repeat for r2, r3, then merge-median as above …
```

## Promote vs docs render

1. Maintainer runs a **median-of-3** publish-set (or study) campaign on
   `maintainer-ws-1` — see **Median merge** above / `make perf-run-publish-set-median`.

2. Promote into `results/references/` (lands via PR — honesty gate).

`promote` **refuses** any run document that still contains `status: invalid`
scenarios (answer gate). After merge-median, strip invalids to an ok-only JSON,
then promote that file. Omitted ids stay out of the reference; study pages show
unavailable poles instead of fabricated QPS. Do not merge an older
`thin-spine.json` that still carries a cell this refresh marked invalid.

```zsh
PYTHONPATH=. python3 <<'PY'
import json
from pathlib import Path
src = Path("perf/results/runs/publish-set-median/median.json")
doc = json.loads(src.read_text())
ok = [sc for sc in doc.get("scenarios") or [] if sc.get("status") == "ok"]
print("ok", len(ok), "omit", [sc["id"] for sc in doc["scenarios"] if sc.get("status") != "ok"])
out = Path("perf/results/runs/publish-set-median/median-promotable.json")
doc = {**doc, "scenarios": ok}
out.write_text(json.dumps(doc, indent=2) + "\n")
print("wrote", out)
PY

PYTHONPATH=. python3 -m perf.runner promote \
  --from perf/results/runs/publish-set-median/median-promotable.json \
  --name thin-spine \
  --publish-set \
  --profile-id maintainer-ws-1 \
  --annotation-id ann-thin-spine-context
# or: make perf-promote PERF_FROM=perf/results/runs/publish-set-median/median-promotable.json
# Legacy thin-spine keep-set: add PERF_PROMOTE_MODE=thin-spine
```

3. Docs/CI generate step renders tables / SVG / CSV from **committed** JSON only —

```zsh
make perf-docs
# or as part of: make docs-build
```

It does **not** invoke load suites or dnsperf.
