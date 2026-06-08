# First query

This page walks through an end-to-end lab test: a client sends a DNS query to Conduit [dataplane](/glossary/index.md#dataplane), Conduit forwards to a [pool](/glossary/index.md#pool) [backend](/glossary/index.md#backend), and the answer returns to the client. It assumes you completed [Install and run](/getting-started/install-and-run.md) and wrote the minimal config from [Minimal configuration](/getting-started/minimal-configuration.md).

The [control plane](/glossary/index.md#control-plane) is **not** required for this exercise — you only need the `conduit` binary, a config file, an upstream resolver, and `dig`.

## What you are proving

| Step | Component |
|------|-----------|
| 1 | Client (`dig`) sends a query to Conduit’s listener |
| 2 | Conduit accepts the query on the [dataplane](/glossary/index.md#dataplane) |
| 3 | Conduit selects the `default` [pool](/glossary/index.md#pool) and forwards to its [backend](/glossary/index.md#backend) |
| 4 | The upstream answers; Conduit returns the response to the client |

For the full pipeline (policy, route, forward), see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

## Lab layout

The [minimal configuration](/getting-started/minimal-configuration.md) example uses loopback ports that avoid UDP **5353** (often mDNS on Linux):

| Role | Address |
|------|---------|
| Conduit DNS listener (UDP) | `127.0.0.1:15353` |
| Pool backend (upstream mock) | `127.0.0.1:5300` |

Use two terminals for Conduit and the upstream, plus a third for `dig` (or reuse one terminal for `dig` after the services are up).

## Prerequisites

- **`dig`** — usually from the `bind9-dnsutils` package on Ubuntu/Debian
- **`dnsmasq`** — lightweight DNS forwarder used here as a loopback upstream mock
- A reachable **recursive resolver** for dnsmasq to forward to (for example `8.8.8.8` or your site resolver)

Set the upstream once per shell session:

```bash
export UPSTREAM_DNS="8.8.8.8"   # replace with a resolver you can reach
```

Save the minimal config as `conduit.yaml` (or use `conduit.minimal.yaml` from a release tarball) and validate it:

```bash
conduitctl validate --file conduit.yaml
```

On success the command prints `ok` and exits with status **0**.

## 1. Start the upstream (backend)

Conduit does not answer from a built-in cache in this setup — something must listen on the pool backend address (`127.0.0.1:5300` in the minimal file). In **terminal A**, start dnsmasq as a forwarder to `$UPSTREAM_DNS`:

```bash
dnsmasq --keep-in-foreground \
  --port=5300 \
  --bind-interfaces \
  --listen-address=127.0.0.1 \
  --server="$UPSTREAM_DNS" \
  --no-hosts --no-resolv --log-queries
```

Leave this process running. If the port is already in use, pick another loopback port, update `pools[].backends[].address` in `conduit.yaml` to match, and re-run `conduitctl validate --file conduit.yaml`.

## 2. Start Conduit

In **terminal B**, start the [dataplane](/glossary/index.md#dataplane) with your config path as the **only** argument after the binary:

```bash
# from a release tarball directory:
./conduit conduit.yaml

# or after building from source:
target/release/conduit conduit.yaml
```

**Expect** a log line similar to:

```text
Starting listening on 127.0.0.1:15353 udp
```

If Conduit exits immediately, check stderr for config errors (see [If the query fails](#if-the-query-fails)).

## 3. Send a query

In **terminal C** (or the same shell once Conduit and dnsmasq are up), query **through Conduit**, not directly against dnsmasq:

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 example.com A
```

`+time=3` and `+tries=1` keep lab failures quick when a backend is down.

### What success looks like

- `dig` status **`NOERROR`**
- An **`ANSWER SECTION`** with at least one A record for `example.com` (exact addresses depend on `$UPSTREAM_DNS`)
- **Terminal A** (dnsmasq) shows a forwarded query in its log output

Short output check:

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 +short example.com A
```

Prints one or more IPv4 addresses when the path is healthy.

## If the query fails

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `connection timed out` / no response | Conduit not listening | Conduit running? Listener address `127.0.0.1:15353` in config? Firewall blocking loopback? |
| `SERVFAIL` | Upstream or forward path | dnsmasq running on `127.0.0.1:5300`? Pool backend address matches? `$UPSTREAM_DNS` reachable from the host? |
| `REFUSED` | Wrong target port | Querying Conduit on **15353**, not dnsmasq on **5300** |
| Conduit exits on start | Invalid config | `conduitctl validate --file conduit.yaml`; fix errors printed to stderr |

Confirm the backend port is open:

```bash
ss -ulnp | grep -E '5300|15353'
```

You should see listeners on both ports when dnsmasq and Conduit are up.

Re-validate after any config edit:

```bash
conduitctl validate --file conduit.yaml
```

## Optional: query with TCP

The minimal example enables **UDP** only. To test TCP, add a TCP listener alongside UDP in `conduit.yaml`:

```yaml
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
    - address: "127.0.0.1:15353"
      protocol: tcp
```

Restart Conduit, then:

```bash
dig @127.0.0.1 -p 15353 +tcp +time=3 +tries=1 example.com A
```

## Next steps

- Tune [pools and backends](/policy-routing/pools-and-backends.md) — weights, multiple pools, selection
- Enable the [control plane](/glossary/index.md#control-plane) for `conduitctl export`, reload, and tracing ([Minimal configuration](/getting-started/minimal-configuration.md#optional-blocks-not-in-this-example))
- Add [metrics](/observability/metrics.md) or [tracing](/observability/tracing.md) when you need observability beyond `dig`

## Related topics

- [Minimal configuration](/getting-started/minimal-configuration.md) — config used in this walkthrough
- [Install and run](/getting-started/install-and-run.md) — install paths and systemd
- [Pools and backends](/policy-routing/pools-and-backends.md) — how Conduit picks a backend
- [Config file](/control-plane/config-file.md) — validation and load behavior
