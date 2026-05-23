# conduit-observation

Phase 2 observation pipeline: bounded per-sink queues, `ObservationHub`, and dnstap export.

## Dependencies (task 4.1 spike)

| Crate | Role |
|-------|------|
| [`dnstap`](https://crates.io/crates/dnstap) 0.1.7 | Protobuf `DNSMessage` / `ClientQuery` / `ClientResponse` encoding |
| [`framestream`](https://crates.io/crates/framestream) 0.2.5 | fstrm `EncoderWriter` over unix/tcp streams |
| `crossbeam-channel` | Bounded non-blocking worker → sink queues |

Conduit connects to an **existing** unix/tcp listener (collector binds first). The `dnstap` crate’s built-in `DNSTapWriter` is not used; we only reuse its protobuf types and encode via `framestream` directly.
