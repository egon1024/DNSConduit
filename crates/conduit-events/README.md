# conduit-events

Phase 2 DNS event export pipeline: bounded per-sink queues, `EventHub`, dnstap export, per-sink metrics, and configurable connect backoff.

## Sink `name`, `export_id`, and `connect_retry`

```yaml
events:
  sinks:
    - type: dnstap
      name: primary-tap          # canonical operator/API id (metrics, future RPCs)
      export_id: conduit-prod    # optional; dnstap wire identity (defaults to name)
      destinations: ["unix:/tmp/dnstap/conduit.sock"]
      emit: [query, response]
      extra_fields: [pool, sink_name]   # sink_name = canonical name in Dnstap.extra JSON
      connect_retry:
        initial_ms: 250
        max_ms: 30000
        multiplier: 2.0
        max_elapsed_ms: 0        # 0 = retry indefinitely
        jitter: true
```

**Identity rules:** provide `name` and/or `export_id`. If only `name` is set, wire identity defaults to `name`. If only `export_id` is set (legacy configs), canonical `name` defaults to `export_id`. Both may differ (e.g. stable `name: prod-tap`, dynamic `export_id: pod-7a3f`). Names and export ids must be unique across sinks.

`CompiledEvents::export_id_for_name`, `name_for_export_id`, and `sink_by_name` resolve between the two at runtime (for metrics/API use).

When the collector is down, the sink worker retries connect with exponential backoff (capped at `max_ms`) instead of a fixed 1s sleep. While disconnected, the hub still enqueues until `queue_depth` is reached; drops increment per-sink `queue_dropped`.

**Logging:** a `warn` when destinations become unreachable or disconnect mid-session; `info` when connectivity is restored; per-retry detail at `debug`; an additional throttled `warn` (`still failing`, about every 60s) while an outage continues.

## Per-sink filters and sampling (phase 2.7)

Each sink may declare `filters` so only matching transactions are enqueued (no wire copy or `extra` build when filtered out).

```yaml
events:
  sinks:
    - type: dnstap
      name: ops-tap
      destinations: ["unix:/tmp/dnstap/ops.sock"]
      emit: [query, response]
      filters:
        selectors:
          - type: qname_suffix
            value: ".corp.example"
          - type: qtype
            value: "A"
        tag_required: audit       # optional; AND with selectors
        sample_percent: 10        # optional; [0, 100]; stable per txn_id (optional sample_key / sample_key_from)
        pool: default             # response/retry only
        backend: "10.0.0.1:53"    # response/retry only
```

Selector types match built-in rules: `qname_suffix`, `qname_exact`, `qtype`, `rcode`, `tag`. **Query** dnstap is emitted after **RequestRules**, so request-phase tags can gate query export.

Fixtures: `tests/fixtures/config/with-dnstap-filters.yaml`, `with-dnstap-sample.yaml`, `with-sample-key.yaml`.

## Per-sink metrics snapshot

`EventHub::sink_metrics_snapshot()` returns in-process counters per sink (`enqueued_*`, `queue_dropped`, `delivered`, `write_failed`, `encode_failed`, `connect_attempts`, `connected`). `dropped_total()` remains the sum of all sinks' `queue_dropped`. Phase 4 will expose these via Prometheus.

## Dependencies (task 4.1 spike)

| Crate | Role |
|-------|------|
| [`dnstap`](https://crates.io/crates/dnstap) 0.1.7 | Protobuf `DNSMessage` / `ClientQuery` / `ClientResponse` encoding |
| `fstrm` (in-crate) | Bidirectional Frame Streams client (`READY`/`ACCEPT`/`START`) over unix/tcp |
| `crossbeam-channel` | Bounded non-blocking worker → sink queues |

Conduit connects to an **existing** unix/tcp listener (collector binds first). The `dnstap` crate’s built-in `DNSTapWriter` is not used; we only reuse its protobuf types and speak Frame Streams via the in-crate `fstrm` module.

## Verifying with `fstrm_capture`

Conduit keeps a long-lived Frame Streams session (handshake once, then data frames per query). That matches `fstrm_capture` and `dnstap-receiver`; the bidirectional `READY` → `ACCEPT` → `START` sequence is required for `fstrm_capture` (unlike `dnstap-receiver`, which also tolerated a `START`-only client).

**Empty or 42-byte capture file while Conduit is running** is usually not a protocol failure. `fstrm_capture -w <file>` writes the file-level `START` control frame immediately (~42 bytes), then buffers per-connection data in stdio until you flush or exit:

```bash
# Terminal 1 — collector (bind socket first)
fstrm_capture -t protobuf:dnstap.Dnstap -u /tmp/dnstap/conduit.sock -w /tmp/dnstap/live.fstrm

# Terminal 2 — Conduit, then generate DNS traffic (e.g. dig)

# Terminal 3 — flush buffered frames to disk, then decode
kill -HUP "$(pgrep -f 'fstrm_capture.*conduit.sock')"
dnstap-ldns -r /tmp/dnstap/live.fstrm -y
```

Stopping `fstrm_capture` with **SIGTERM** (Ctrl+C) also flushes. `kill -9` or reading the file without flushing first often leaves only the 42-byte file header.

For **live** console output, do not use `fstrm_capture -w - | dnstap-ldns`. `fstrm_capture` only pushes connection data to its output after a large read batch (default 256 KiB on the socket) or on flush/exit, so a long-lived Conduit session produces no pipe output per query.

For live decode **including `extra`**, use the in-repo development tool `conduit-dnstap-tracer` (streams each frame to stdout as it arrives):

```bash
cargo build -p conduit-dnstap-tracer
./target/debug/conduit-dnstap-tracer -u /tmp/dnstap/conduit.sock -f yaml
# formats: log (default), json, yaml; add --unidirectional for START-only clients
# decodes DNS header, question (type/class), RR sections, socket metadata, timestamps, and extra JSON
```

The Go `dnstap` CLI (package `golang-github-dnstap-golang-dnstap-cli`) also works for live DNS decode but does not print `extra`:

```bash
# Terminal 1 — collector + live YAML (only one process may own the socket)
rm -f /tmp/dnstap/conduit.sock
dnstap -u /tmp/dnstap/conduit.sock -y

# Terminal 2 — Conduit (destinations: unix:/tmp/dnstap/conduit.sock), then dig
```

Quiet text: `dnstap -u /tmp/dnstap/conduit.sock -q`. Save raw frames too: `dnstap -u /tmp/dnstap/conduit.sock -w /tmp/dnstap/live.fstrm -y`.

### `extra_fields` in the console

Conduit stores configured metadata as JSON in the dnstap protobuf `extra` field (see `extra_fields` / `extra_tags` in config). The Go `dnstap` tool’s `-y`, `-q`, and `-j` formatters **do not print** `extra` (upstream limitation in golang-dnstap).

To confirm extras are present:

```bash
strings /tmp/dnstap/live.fstrm | grep -E '"pool"|"backend"'
```

Look at **client response** (`CR`) lines for `pool` / `backend`; **client query** (`CQ`) events are emitted before routing, so those fields are often absent on queries.

`dnstap-ldns` may or may not print `extra` depending on version; the `strings` check above is the reliable confirmation.
