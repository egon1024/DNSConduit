# conduit-observation

Phase 2 observation pipeline: bounded per-sink queues, `ObservationHub`, and dnstap export.

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

For live decode **including `extra`**, use the in-repo dev tool `conduit-dnstap-tap` (streams each frame to stdout as it arrives):

```bash
cargo build -p conduit-dnstap-tap
./target/debug/conduit-dnstap-tap -u /tmp/dnstap/conduit.sock -f yaml
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
