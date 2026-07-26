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

Re-render without re-running:

```zsh
python3 -m perf.runner render --from perf/results/runs/run-….json --format fancy
python3 -m perf.runner render --from perf/results/runs/run-….json --format html -o /tmp/perf.html
```

## Primary loadgen (dnsperf)

| Aspect | Value |
|--------|-------|
| Tool | DNS-OARC [dnsperf](https://github.com/DNS-OARC/dnsperf) |
| Default | Docker image `dnsconduit-dnsperf:2.14.0` built from `perf/fixtures/dnsperf/Dockerfile` (upstream **2.14.0**) |
| Network | `--network=host` so dnsperf reaches **127.0.2.1:15353** |
| Query file | `perf/fixtures/queries/perf-a.txt` |
| Typical flags | `-s 127.0.2.1 -p 15353 -d <queries> -c 4 -T 2 -l <seconds>` |
| Override | `--loadgen-mode native` when `dnsperf` is on `$PATH` |

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
| `catalog/lab_profiles/` | Named host profiles (template + filled instances) |
| `catalog/annotations/` | Stable-id footnotes (tone, title, body) |
| `runner/` | Python CLI (`python3 -m perf.runner`) |
| `fixtures/` | Conduit configs, query files, dnsperf Dockerfile, upstream recipes |
| `helpers/` | Pointers to companion Rust lab binaries |
| `results/schema.json` | Canonical run document schema |
| `results/runs/` | Append-oriented run JSON |
| `results/references/` | Curated promoted snapshots (manual PR) |
| `render/` | plain / fancy / yaml / json / html from run JSON |

## Suites

| Suite | Focus |
|-------|-------|
| `scale` | Runtime models × load shapes (`forward_fast`, `forward_slow`, `cache_hit`) |
| `shutdown_drain` | Three drain policies (`drain_complete` / `drain_budgeted` / `drain_minimal`) under `forward_slow` load; records `drain_duration_ms` and `client_failures_during_stop` |
| `feature_tax` | Metrics ladder (off / minimal / standard scrape), collect vs emit pairs, dnstap off/sampled/fuller, OTLP push (skips until `conduit-otlp-metrics-tracer`) |
| `lifecycle` | Cold start to first answer; thin config apply via `conduitctl` |
| `lossless_upgrade` | Gated on zero-downtime upgrade — skipped until available |

## Lab ports

Matches the maintainer lab map: Conduit DNS **127.0.2.1:15353**, stub upstream
**127.0.2.1:15300**, control **127.0.2.1:5199**, Prometheus scrape **127.0.2.1:19090**,
dnstap socket **unix:/tmp/conduit-perf-dnstap.sock**.

## Make targets

```zsh
make perf-unit          # harness unit tests (no live loadgen)
make perf-list          # list catalog
make perf-run-scale     # run scale suite (requires CONDUIT=)
make perf-run-shutdown-drain  # run shutdown_drain suite
make perf-run-feature-tax     # run feature_tax suite
make perf-run-lifecycle       # run lifecycle suite
make perf-render        # render FROM=… FORMAT=plain|fancy|yaml|json|html
```

`make performance` remains the **microbench** (Rhai Criterion) path and is distinct.

## Promote vs docs render

1. Maintainer runs suites on the blessed lab profile and lands run JSON via PR under
   `results/references/`.
2. Docs/CI generate step renders tables / SVG / CSV from **committed** JSON only —
   it does **not** invoke load suites.
