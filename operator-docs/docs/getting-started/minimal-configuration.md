# Minimal configuration

This page shows the smallest [config file](/control-plane/config-file.md) that can start the [dataplane](/glossary/index.md#dataplane), accept DNS queries, and forward them to an upstream resolver. Use it when standing up a lab or following [Install and run](/getting-started/install-and-run.md).

The [control plane](/glossary/index.md#control-plane) (gRPC and `conduitctl`) is **opt-in**: it is **not** started unless you add an explicit `control:` block. The minimal example below does not enable it.

**Minimal** means the fewest blocks you must author. Conduit fills in safe defaults for everything else at load time. This page covers only the three required blocks; field-level reference and tuning are described in [Reference: config schema](/reference/config-schema/index.md) and the linked topic pages below.

## Minimal example

Save the file below (for example as `conduit.yaml`). Lab examples use **`127.0.0.1:15353`** for Conduit’s DNS listener (avoiding UDP **5353**, which is often mDNS on Linux). The pool points at **`127.0.0.1:5300`** — something must be listening there (for example a mock resolver or dnsmasq forwarding to a public resolver) before queries succeed.

```yaml
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
```

That is a complete, runnable configuration. You do not need to declare `forward`, `orchestrator`, `events`, `rhai`, or `control` unless you want to change their defaults.

## What each block does

### `schema_version`

Required top-level key. The only accepted value is **`1`**. Omitting the key fails YAML parsing; any other value fails [validation](/control-plane/config-file.md).

### `listeners`

[Dataplane](/glossary/index.md#dataplane) **ingress**: where clients send DNS queries. You need at least one entry under `listeners.listeners` with an `address` (`ip:port`) and `protocol` (`udp` or `tcp`).

When you omit `listeners.threads` and `listeners.reuse_port`, Conduit uses **1** thread and **`reuse_port: false`**. For every listener field, default, and validation rule, see [Reference: listeners](/reference/config-schema/listeners.md).

### `pools`

Where Conduit **forwards** queries. Each [pool](/glossary/index.md#pool) has a unique `name` and at least one [backend](/glossary/index.md#backend) (`address: "ip:port"`). The name `default` is a convention for the catch-all pool when nothing else selects one.

Omitted `weight` on a backend defaults to **100** for load balancing. For selection behavior, multiple pools, and retries, see [Pools and backends](/policy-routing/pools-and-backends.md). For every pool and backend field, see [Reference: pools](/reference/config-schema/pools.md).

## Defaults you do not need to write yet

Conduit still loads and applies these blocks when they are absent from your file. You can add them later when you need to tune behavior.

| Block {: .column-no-wrap } | What Conduit applies when omitted | Learn more |
|-------|-----------------------------------|------------|
| `forward` | Upstream timeout **2000** ms, **100** outstanding queries per backend, UDP-only transport | [Reference: forward](/reference/config-schema/forward.md), [Dual-stack forwarding](/guides/dual-stack-forwarding.md) |
| `orchestrator` | **3** max attempts, **5000** ms max [transaction](/glossary/index.md#transaction) duration, **1024** transaction table capacity | [Reference: orchestrator](/reference/config-schema/orchestrator.md), [Retries and transactions](/policy-routing/retries-and-transactions.md) |
| `control` | **Off** when omitted — no gRPC listener; add a `control:` block with `listen_address` to enable [conduitctl](/control-plane/grpc-and-conduitctl.md) | [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md), [Reference: control](/reference/config-schema/control.md) |
| `events` | Queue depth **4096**, **`drop_oldest`** policy, no sinks | [Event export](/observability/event-export.md), [Reference: events](/reference/config-schema/events.md) |
| `rhai` | Sandbox limits (**10000** operations, call depth **32**); no scripts unless you add them | [Rhai](/rhai/index.md), [Sandbox limits](/rhai/sandbox-limits.md) |

To see the effective configuration after defaults are applied, run **`conduitctl validate --file conduit.yaml`** (offline validation and snapshot compile — no running server required), or follow [Validate and run](#validate-and-run). To export normalized YAML from a **running** server with the control plane enabled, use **`conduitctl export`** — see [Configuration model](/control-plane/configuration-model.md) and [Reload and export](/control-plane/reload-and-export.md).

## Optional blocks not in this example

You can add these once the baseline works — none are required to start Conduit:

- **[Rules](/policy-routing/rules-and-actions.md)** — policy routing before forward
- **[Backend health](/policy-routing/backend-health.md)** — optional per-pool probes and passive fast-trip (`pools[].health`; disabled by default)
- **[Metrics](/observability/metrics.md)** and **[tracing](/observability/tracing.md)** — observability
- **[Event export](/observability/event-export.md)** (dnstap sinks) — requires `events.sinks`
- **[API keys](/security/api-keys.md)** and **[mTLS](/security/mtls.md)** — control-plane security (requires an explicit `control:` block)

To enable the [control plane](/glossary/index.md#control-plane), add for example:

```yaml
control:
  listen_address: "127.0.0.1:5199"
```

Changing or adding `control:` via reload requires a **process restart** today; see [Reload and export](/control-plane/reload-and-export.md).

For the full query path (listen → policy → Lookup → send), see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

## Validate and run

1. **Validate** the file locally (no running server required):

   ```bash
   conduitctl validate --file conduit.yaml
   ```

   On success the command prints `ok`. Parse and validation errors are printed to stderr.

2. **Start Conduit** with the config path as the first argument (see [Install and run](/getting-started/install-and-run.md) for build prerequisites):

   ```bash
   target/release/conduit conduit.yaml
   ```

3. **Send a test query** once an upstream is listening on the pool address (see [First query](/getting-started/first-query.md)):

   ```bash
   dig @127.0.0.1 -p 15353 +time=3 +tries=1 example.com A
   ```

For file format, load behavior, and reload via the [control plane](/glossary/index.md#control-plane), see [Config file](/control-plane/config-file.md).

## Related topics

- [Install and run](/getting-started/install-and-run.md) — build, start Conduit, prerequisites
- [First query](/getting-started/first-query.md) — send a test query through the minimal setup
- [Config file](/control-plane/config-file.md) — file format, validation, and load behavior
- [Pools and backends](/policy-routing/pools-and-backends.md) — pool selection and backend weights
- [Reference: config schema](/reference/config-schema/index.md) — field reference for every block
