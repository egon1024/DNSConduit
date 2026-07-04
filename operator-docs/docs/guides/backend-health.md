# Backend health

Enable per-pool health, watch probes mark a dead backend down, and practice a maintenance [drain](/glossary/index.md#drain) with **`conduitctl health`**. Behavioral detail: [Backend health](/policy-routing/backend-health.md). Config fields: [Reference: health](/reference/config-schema/health.md).

**Prerequisites:** Conduit installed ([Install and run](/getting-started/install-and-run.md)); a working baseline ([Minimal configuration](/getting-started/minimal-configuration.md)); **`control:`** at process start so **`conduitctl health`** works; **`metrics.profile: full`** if you want health gauges and probe counters.

## Lab layout

| Role | Address |
|------|---------|
| Conduit DNS | `127.0.0.1:15353` |
| Live upstream | `127.0.0.1:5300` (must answer DNS) |
| Dead backend | `127.0.0.1:5399` (nothing listening) |
| Control plane | `127.0.0.1:5199` |
| Prometheus scrape | `http://127.0.0.1:9090/metrics` |

Point `127.0.0.1:5300` at a local resolver or mock (for example dnsmasq) before starting Conduit.

## Enable health

Save as `conduit-health.yaml` (adjust the live backend if your upstream is elsewhere):

```yaml
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    health:
      enabled: true
      interval_ms: 1000
      rise: 3
      fall: 2
      passive_fast_trip: true
      passive_fall: 2
    backends:
      - address: "127.0.0.1:5300"
        name: live
        weight: 100
      - address: "127.0.0.1:5399"
        name: dead
        weight: 100
control:
  listen_address: "127.0.0.1:5199"
metrics:
  profile: full
  prometheus:
    listen_address: "127.0.0.1:9090"
```

Validate and start:

```bash
conduitctl validate --file conduit-health.yaml
conduit conduit-health.yaml
```

## Watch probes mark `dead` down

With defaults, about **`fall × interval_ms`** (here ~2 s) of failed probes marks **`dead`** down. **`live`** stays up.

```bash
conduitctl health show
# or filter:
conduitctl health show --pool default --backend dead
```

Expect **`dead`**: observed and applied **down**, not eligible. **`live`**: **up** and eligible.

With metrics enabled:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep -E 'conduit_backend_health_applied|conduit_probe_results|conduit_pool_backends_active'
```

Process logs emit `backend health transition` at INFO when probes change observed/applied state.

Send client traffic — [Route](/concepts/architecture-and-packet-path.md#route) should prefer **`live`** only:

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 example.com A
```

## Passive fast-trip (optional)

With **`passive_fast_trip: true`**, live forward timeouts and hard errors can mark a backend down before probe **`fall`** completes. Under load toward a failing backend, watch WARN lines `passive health: forward failure` and `passive fast-trip: backend marked down`. Passive alone **cannot** mark a backend **up** again — only probe **`rise`** (or operator **`health set up`** / **`health resume`**) restores eligibility.

## Maintenance drain

Take **`live`** out of rotation without stopping probes:

```bash
conduitctl health set down --pool default --backend live
conduitctl health show --pool default --backend live
```

Applied is **down** and the scope is [frozen](/glossary/index.md#freeze); observed may still be **up**. With **`dead`** already down and default **`min_eligible: 0`**, no backend is eligible — client queries get **SERVFAIL** (fail-open does not apply). That is expected for this lab; in production you typically drain one backend while others stay up.

Return to probe-driven routing:

```bash
conduitctl health resume --pool default --backend live
conduitctl health show --pool default
```

**`health resume`** unfreezes and snaps **applied** to **observed** in one step — prefer it over ad-hoc clear/freeze sequences ([Clear-while-frozen](/policy-routing/backend-health.md#clear-while-frozen-footgun)). Client `dig` should succeed again once **`live`** is applied **up**.

## What to verify

| Check | Expected |
|-------|----------|
| `health show` after ~few seconds | **`dead`** applied down; **`live`** up |
| Client `dig` | Answers via **`live`** (not timeouts to **`dead`**) |
| Metrics (`full`) | `conduit_backend_health_*`, `conduit_probe_results_total` present |
| After `set down` on **`live`** | Applied down, frozen; probes still update observed |
| After `health resume` | Applied tracks observed again |

## Related topics

- [Backend health](/policy-routing/backend-health.md) — mental model, fail-open, reload preservation
- [gRPC and conduitctl — health](/control-plane/grpc-and-conduitctl.md#health) — command reference
- [Built-in metrics — Backend health](/observability/built-in-metrics.md#backend-health)
- [Runtime API](/rhai/runtime-api.md) — script reads via **`runtime.routing()`**
- [Troubleshooting — Backend health](/troubleshooting/index.md#backend-health)
