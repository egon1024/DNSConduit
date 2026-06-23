# Unreleased

## New features

- **Operator documentation** — published manual at [https://egon1024.github.io/DNSConduit/](https://egon1024.github.io/DNSConduit/) covering getting started, architecture, policy and routing, control plane, Rhai scripting, observability, guides, reference, glossary, and troubleshooting. Versioned snapshots deploy with each release tag.
- **Release notes** — per-version operator release notes on the docs site; contributors stage bullets in this file before each release. See [Release notes](/release-notes/index.md).
- **Release artifacts** — GitHub Releases can ship Linux `amd64` tarballs, Debian packages, `SHA256SUMS`, and an SPDX SBOM. See [Install and run](/getting-started/install-and-run.md).
- **Apache 2.0 license** — project licensed under Apache 2.0 with DCO sign-off for contributions.
- **Declarative rules** — expanded selectors and actions, including ordered action lists, `set_source` on request rules, and `set_retry_source` for retry egress. See [Rules and actions](/policy-routing/rules-and-actions.md).
- **Rhai scripting** — broader transaction and DNS wire API (including IANA class/type/rcode enums), user-defined metrics from scripts, lookup-table support, and compile-time validation of scripts on config reload. See [Rhai](/rhai/index.md) and [Transaction API](/rhai/transaction-api.md).
- **Metrics profiles** — `metrics.profile: minimal` (default-style volume counters) vs `full` (richer labels, phase timing, and Linux process gauges). See [Built-in metrics](/observability/built-in-metrics.md) and [Operator metrics profiles](/guides/operator-metrics-profiles.md).
- **OpenTelemetry metrics export** — OTLP push for built-in metrics when `metrics.otel` is configured. See [Metrics](/observability/metrics.md).
- **Event export** — improved dnstap and structured event filtering, selectors, and extra-field handling. See [Event export](/observability/event-export.md).
- **Control plane overlay** — `conduitctl apply` merges patches into the accumulated overlay by default; `--replace` and `--clear` remain available. Patches that include file-only sections (`rules`, `metrics`, `tracing`) are rejected at apply time. See [Configuration model](/control-plane/configuration-model.md) and [Reload and export](/control-plane/reload-and-export.md).
- **Sample keys** — optional `sample_key` on listeners and rules for trace and event sampling. See [Tracing](/observability/tracing.md).

## Improvements

- **Quieter default logging** — per-query log lines are at `debug`; default `info` emphasizes lifecycle and control-plane events. See [Logging](/observability/logging.md).
- **Retry behavior** — clearer same-pool retry semantics and transaction limits. See [Retries and transactions](/policy-routing/retries-and-transactions.md).
- **Config paths** — relative paths for TLS material and other file references resolve from the config file directory. `conduitctl validate` checks path resolution.
- **Logging format** — removed ASCII control-character rendering from log output.
- **Development tracer** — dnstap decode utility renamed to `conduit-dnstap-tracer`.

## Upgrade notes

- Review [Minimal configuration](/getting-started/minimal-configuration.md) if you rely on implicit defaults — the control plane and metrics export remain opt-in; a sparse config still needs only `schema_version`, `listeners`, and `pools`.
- After upgrading, validate YAML with `conduitctl validate --file …` and reload or restart as usual. Rhai syntax errors in scripts now fail reload instead of being deferred to query time.
- If you depend on verbose per-query process logs at `info`, raise `logging.level` to `debug` or use metrics and event export for traffic visibility.
