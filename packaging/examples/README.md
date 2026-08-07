# Packaged example configs

Shipped with production tarballs and the `conduit` Debian package under
`/usr/share/doc/conduit/examples/`. Each lab subdirectory matches a **Guides**
page in the operator documentation (same YAML / scripts as the walkthrough).

| Path | Guide topic |
|------|-------------|
| `conduit.minimal.yaml` | Minimal configuration (also `/etc/conduit/conduit.yaml`) |
| `conduit.reference.yaml` | Field tour — not intended to run unchanged |
| `backend-health/` | Backend health |
| `dns-answer-cache/` | DNS answer cache (`conduit-cache.yaml` memory; `conduit-cache-lmdb.yaml` LMDB) |
| `declarative-failover/` | Declarative failover |
| `rule-action-order/` | Rule action order (includes `set-vip-pool.rhai`) |
| `metrics-and-tracing/` | Metrics and tracing |
| `operator-metrics-bases/` | Operator metrics bases |
| `metrics-beyond-bases/` | Metrics beyond bases |
| `event-export-dnstap/` | Event export and dnstap |
| `otlp-metrics-push/` | OTLP metrics push smoke |
| `rhai-policy/blocklist/` | Rhai policy (CSV blocklist lab) |

Lab defaults use loopback DNS on **`127.0.0.1:15353`** and upstream
**`127.0.0.1:5300`** unless a guide says otherwise. Validate before run:

```bash
conduitctl validate --file /path/to/example.yaml
```
