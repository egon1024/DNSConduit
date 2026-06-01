# Manual test guide — phase 5 control plane (hot reload)

> **Repository:** DNSConduit root. **Ports:** lab range avoids UDP **5353** (mDNS on many Linux desktops).
>
> Datapath / metrics baseline: [`phase-4-metrics-tracing.md`](phase-4-metrics-tracing.md), [`phase-4b-operator-metrics.md`](phase-4b-operator-metrics.md).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Upstream mock (dnsmasq) | `127.0.0.1:15300` → `$UPSTREAM_DNS` |
| Control gRPC | `127.0.0.1:5199` |

## Configs (this guide)

| Purpose | File |
|---------|------|
| **Baseline** — file layer at startup | [`config/phase-5-base.yaml`](config/phase-5-base.yaml) |
| **Overlay** — pool weight 100 → 50 | [`config/phase-5-overlay-weight.yaml`](config/phase-5-overlay-weight.yaml) |
| **Overlay (invalid)** — `listeners.threads: 0` | [`config/phase-5-overlay-invalid.yaml`](config/phase-5-overlay-invalid.yaml) |
| **Overlay** — listener thread bump (pending reconcile) | [`config/phase-5-overlay-listeners.yaml`](config/phase-5-overlay-listeners.yaml) |
| **Overlay** — enable API keys | [`config/phase-5-overlay-api-keys.yaml`](config/phase-5-overlay-api-keys.yaml) |
| **Tracing** — `conduitctl trace` smoke | [`config/phase-5-tracing.yaml`](config/phase-5-tracing.yaml) |

**Note:** `conduitctl apply --file` loads a **full** YAML document. Overlays in this guide are complete configs that differ from the base only where noted.

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit -p conduitctl --release
export UPSTREAM_DNS=8.8.8.8   # or your resolver
chmod +x tests/manual/scripts/check-ports.sh
tests/manual/scripts/check-ports.sh
ss -tln | grep 5199 || echo "5199 appears free"
```

Tools: `dig`, `dnsmasq`, `grpcurl`, `rg` (or `grep`).

Environment (optional defaults):

```bash
export CONDUIT_CONTROL=http://127.0.0.1:5199
# export CONDUIT_API_KEY=phase5-manual-test-key   # section 9 only
```

**Shell helpers (zsh and bash):** zsh does **not** word-split `export VAR="cargo run …"` — `$VAR args` is treated as one command name. Use **functions** (or run full commands / release binaries):

```bash
# After: cargo build -p conduit -p conduitctl --release
# Option A — release binaries (simplest):
#   ./target/release/conduit tests/manual/config/phase-5-base.yaml
#   ./target/release/conduitctl apply --file tests/manual/config/phase-5-overlay-weight.yaml

# Option B — functions (works in zsh and bash):
ctl() { cargo run -p conduitctl --quiet -- "$@"; }
run-conduit() { cargo run -p conduit --quiet -- "$@"; }
```

The rest of this guide uses `ctl …` and `run-conduit …`.

## Terminal layout

| Terminal | Role |
|----------|------|
| **A** | dnsmasq on `15300` |
| **B** | Conduit (watch logs here) |
| **C** | `conduitctl` / `grpcurl` / `dig` |

---

## 0. Start upstream (Terminal A)

```bash
dnsmasq --keep-in-foreground \
  --port=15300 \
  --bind-interfaces \
  --listen-address=127.0.0.1 \
  --server="$UPSTREAM_DNS" \
  --no-hosts --no-resolv --log-queries
```

---

## 1. Health and baseline DNS

**Terminal B:**

```bash
run-conduit tests/manual/config/phase-5-base.yaml
```

**Expect in stderr/logs:**

- `Starting listening on 127.0.0.1:15353 udp` (or equivalent listener line)
- `starting control plane` with `addr=127.0.0.1:5199`

**Terminal C:**

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/Health
dig @127.0.0.1 -p 15353 +time=3 +tries=1 phase5.example.com A
```

**Expect:**

- Health: `"status": "serving"`
- `dig` returns an answer (NOERROR) when dnsmasq is up
- **Terminal B** includes an access log line, for example:
  `control rpc rpc=conduit.v1.ConduitControl/Health peer=127.0.0.1:… requestor=anonymous grpc_code=Ok latency_ms=…`

---

## 1b. Control RPC access log (no payloads)

Phase **5** logs every `ConduitControl` RPC at `info` as `control rpc` with:

| Field | Meaning |
|-------|---------|
| `rpc` | gRPC method, e.g. `conduit.v1.ConduitControl/Health` |
| `peer` | Client socket address, or `unknown` |
| `requestor` | `anonymous`, `api_key`, `api_key_rejected`, `mtls`, or `unauthenticated` (never the raw API key) |
| `grpc_code` | gRPC status code |
| `latency_ms` | Round-trip time |

Request/response bodies are **not** logged. Config changes also emit `config applied` (see section 3).

**Terminal C** — trigger and watch **Terminal B**:

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/Health
```

**Expect:** one `control rpc` line with `requestor=anonymous` when `api_keys` are unset in the active config.

---

## 2. `GetConfig` — effective config matches file baseline

**Terminal C** (Conduit still on `phase-5-base.yaml`):

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/GetConfig \
  | rg 'weight|15300|threads'
```

**Expect:** backend weight **100**, upstream **127.0.0.1:15300**, listeners threads **2**.

---

## 3. `ApplyConfig` — overlay changes pool weight

**Terminal C:**

```bash
ctl apply --file tests/manual/config/phase-5-overlay-weight.yaml
```

**Expect:**

- CLI prints `ok`
- **Terminal B** log line similar to: `config applied` with `source=grpc` and `generation=1` (generation ≥ 1)
- Optional diff line: `pool: backends changed`

Verify effective config:

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/GetConfig \
  | rg '"weight": 50'
```

**Expect:** `"weight": 50` on the default pool backend.

Confirm DNS still works:

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 phase5-after-apply.example.com A
```

---

## 4. `ExportConfig` — export reflects overlay

**Terminal C:**

```bash
ctl export | rg 'weight'
```

**Expect:** exported YAML shows backend `weight: 50` (effective = file + overlay).

Save to disk:

```bash
ctl export --output /tmp/conduit-effective.yaml
rg weight /tmp/conduit-effective.yaml
```

---

## 5. Rejected overlay — last-good snapshot retained

**Terminal C:**

```bash
ctl apply --file tests/manual/config/phase-5-overlay-invalid.yaml ; echo exit=$?
```

**Expect:**

- Non-zero exit / error mentioning validation (e.g. `threads`)
- **No** new `config applied` success line in Terminal B (or apply errors logged)

Effective weight still **50** from section 3:

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/GetConfig | rg '"weight":'
```

**Expect:** still `"weight": 50`, not reverted to 100 and not broken.

---

## 6. `ReloadFromFile` — file wins, overlay cleared

With overlay still active (weight 50), **Terminal C:**

```bash
ctl reload
```

**Expect:**

- CLI prints `ok`
- Terminal B: `config applied` with `source=file` (or `file` in source field)

Effective config back to file baseline:

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/GetConfig | rg '"weight": 100'
ctl export | rg 'weight'
```

**Expect:** `GetConfig` shows weight **100**. `ctl export` may omit `weight:` when the value is the default (**100**) — that is normal export normalization, not a failed reload.

---

## 7. SIGHUP — same semantics as reload

**Terminal C** — re-apply overlay:

```bash
ctl apply --file tests/manual/config/phase-5-overlay-weight.yaml
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/GetConfig | rg '"weight": 50'
```

Send SIGHUP to Conduit (**Terminal B** PID):

```bash
# Terminal C — find PID (adjust if you run release binary directly)
pgrep -f 'conduit.*phase-5-base.yaml'
kill -HUP <pid>
```

**Expect:**

- Terminal B: `config applied` with `source=sighup`
- Effective weight **100** again:

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/GetConfig | rg '"weight": 100'
```

**Optional file-edit variant:** while overlay is active, change `weight:` in `tests/manual/config/phase-5-base.yaml` on disk to **75**, then `kill -HUP`. **Expect** effective weight **75** (overlay cleared, new file layer read).

Restore `phase-5-base.yaml` to `weight: 100` before later sections.

---

## 8. Pending reconcile — listener change without socket rebind

**Terminal C** (start from file baseline — reload or restart Conduit if needed):

```bash
ctl apply --file tests/manual/config/phase-5-overlay-listeners.yaml
```

**Expect in Terminal B logs:**

- `config applied` …
- `listeners: pending (restart required) — snapshot updated, sockets not rebound`

**Terminal C** — DNS should still answer (same UDP socket):

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 pending-reconcile.example.com A
```

Restart Conduit to pick up listener thread changes (full reconcile is post–v1).

---

## 9. API key auth (hot-applied)

From open control (no keys), **Terminal C:**

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/Health
```

**Expect:** success.

Apply auth overlay:

```bash
ctl apply --file tests/manual/config/phase-5-overlay-api-keys.yaml
```

Unauthenticated call should fail:

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/Health ; echo exit=$?
```

**Expect:** `Unauthenticated` / non-zero exit. **Terminal B:** `control rpc` with `requestor=api_key_rejected` or `unauthenticated` and `grpc_code=Unauthenticated`.

With key:

```bash
export CONDUIT_API_KEY=phase5-manual-test-key
ctl export | head -5
grpcurl -plaintext \
  -H "authorization: Bearer phase5-manual-test-key" \
  127.0.0.1:5199 conduit.v1.ConduitControl/Health
```

**Expect:** both succeed.

Clear auth for later runs: `ctl reload` (returns to `phase-5-base.yaml` without `api_keys`).

---

## 10. `conduitctl validate` (local, no server)

**Terminal C** (Conduit may be stopped):

```bash
ctl validate --file tests/manual/config/phase-5-base.yaml ; echo ok=$?
ctl validate --file tests/manual/config/phase-5-overlay-invalid.yaml ; echo bad=$?
```

**Expect:** first exits 0 / prints `ok`; second fails with validation errors.

---

## 11. `conduitctl trace`

Stop Conduit on **Terminal B**. Restart with tracing config:

```bash
run-conduit tests/manual/config/phase-5-tracing.yaml
```

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 trace-phase5.example.com A
ctl trace 1
```

**Expect:** phase lines including `route` and/or `forward` (first transaction id is usually `1`).

Unknown id:

```bash
ctl trace 999999 ; echo exit=$?
```

**Expect:** non-zero / “trace not found”.

---

## 12. TLS / mTLS (optional)

Not covered by the YAML fixtures above. To smoke-test TLS locally:

```bash
mkdir -p /tmp/conduit-phase5-tls
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -keyout /tmp/conduit-phase5-tls/key.pem \
  -out /tmp/conduit-phase5-tls/cert.pem \
  -subj '/CN=localhost'
```

Add to a copy of `phase-5-base.yaml`:

```yaml
control:
  listen_address: "127.0.0.1:5199"
  reflection_enabled: true
  tls:
    cert_path: "/tmp/conduit-phase5-tls/cert.pem"
    key_path: "/tmp/conduit-phase5-tls/key.pem"
```

Restart Conduit with that file. Client calls must use TLS, e.g.:

```bash
grpcurl -insecure 127.0.0.1:5199 conduit.v1.ConduitControl/Health
export CONDUIT_CONTROL=https://127.0.0.1:5199
# conduitctl may require additional TLS trust configuration for production CA/mTLS
```

For **mTLS**, generate a client CA, set `client_ca_path` under `control.tls`, and present a client certificate — same pattern as design §8.2.

---

## 13. Automated regression (sanity)

```bash
make test
cargo test -p conduit-api --test grpc_apply_config
cargo test -p conduit-core configurator::
```

---

## Troubleshooting

| Symptom | Check |
|---------|--------|
| `connect: connection refused` on `:5199` | Conduit running? `ss -tln \| grep 5199` |
| `apply failed: reading config` | Path to overlay YAML correct from repo root |
| `dig` SERVFAIL | dnsmasq on `15300`? `$UPSTREAM_DNS` reachable? |
| `Unauthenticated` unexpectedly | `ctl reload` or restart without `api_keys` overlay |
| Overlay weight not visible in `GetConfig` | Apply succeeded? Check Terminal B for `config applied` |
| SIGHUP no effect | Signal sent to Conduit PID, not shell parent; Linux only in v1 |

---

## Cleanup

```bash
# Terminal B: Ctrl+C
# Terminal A: Ctrl+C
unset CONDUIT_API_KEY CONDUIT_CONTROL
# unset -f ctl run-conduit   # if you defined the shell functions
```

Restore `tests/manual/config/phase-5-base.yaml` if you edited it during section 7.
