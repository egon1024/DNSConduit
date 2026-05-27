# conduit-script

Rhai scripting for DNS Conduit: compile scripts at snapshot build, run at RequestRules / ResponseRules when a matching rule includes `action: rhai`.

## Execution model

- **Compile at snapshot build:** script source is read once, compiled to `AST`, and stored in `CompiledScripting` with shared `Arc<DataSourceStore>` and a monotonic `snapshot_generation`.
- **One Rhai `Engine` per datapath worker OS thread:** a thread-local `ScriptRuntime` registers the host API once per snapshot generation. Config reload bumps `snapshot_generation` and rebuilds that thread's engine on the next script run.
- **Per hook invocation:** fresh `Scope`, fresh `txn` bindings, per-run sandbox limits (`max_operations`, `max_call_depth`, `hook_timeout_ms`), and a thread-local pointer to the snapshot's data sources for `table_lookup`. The precompiled `AST` is executed; the engine is not recreated.
- **Default path:** if no matching rule has `action: rhai`, Rhai is not invoked (no interpreter work on that query).

## Transaction API (`txn`)

| Method | Phase | Notes |
|--------|-------|-------|
| `question()` | request | Map: `qname`, `qtype`, `id` |
| `response()` | response | Map: `rcode`, `qname`, `qtype`; errors in request phase |
| `set_tag(key, value)` | both | bool or string |
| `has_tag(key)` | both | |
| `set_pool(name)` | request | |
| `set_rd(bool)` | request | upstream RD bit override |
| `clear_rd()` | request | alias for `set_rd(false)` |
| `set_source_v4(addr)` | request | IPv4 egress; must be in configured `sources_v4` for pool |
| `set_source_v6(addr)` | request | IPv6 egress; must be in configured `sources_v6` for pool |
| `set_retry_pool(name)` | response | sets retry pool |
| `drop_query()` | request | terminate without forward |
| `set_rcode(name)` | response | |
| `table_lookup(table, key)` | both | global; host-loaded `data_sources` only |
| `question_qname(txn)` | both | global helper |
| `sample_include(rate)` | both | deterministic on txn id; may set `sampled` tag |
| `metric_inc(name, delta)` | both | registered at script load |
| `metric_inc_labels(name, delta, labels)` | both | bounded label keys only |
| `elapsed_ms()` | response | |
| `get_attempt_count()` | response | use this name in scripts (not `attempt_count()`) |

## Sandbox

From config `rhai:` section:

- `max_operations` — Rhai operation budget per hook
- `max_call_depth` — call stack depth
- `hook_timeout_ms` — wall time per hook (default 50ms)

On trap, timeout, or limit exceeded: log warning, increment script error counter, **fail-open** (forward path continues without further script effects for that hook).

## Performance check (local)

Ignored micro-benchmark for thread-local reuse:

```bash
cargo test -p conduit-script thread_local_runtime_bench -- --ignored --nocapture
```

Sample run (Linux x86_64, release, warm thread-local engine, minimal VIP-pool script): 10k invocations in ~10ms (~1M runs/sec). Compare on your hardware; debug builds are much slower.

## Examples

See [tests/fixtures/rhai/README.md](../../tests/fixtures/rhai/README.md).
