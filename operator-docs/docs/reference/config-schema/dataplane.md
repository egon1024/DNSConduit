# Config schema: dataplane

Field reference for the top-level **`dataplane:`** block — the **dataplane runtime model** Conduit uses to spread the per-query [pipeline](/concepts/architecture-and-packet-path.md#pipeline-phases) across OS threads. For the behavior in context — what each runtime does, the worker roles, and the slot pool — see [Runtime and concurrency](/concepts/runtime-and-concurrency.md).

The runtime is chosen **once at process startup**. Changing `dataplane:` requires a **restart** to take effect; it does not apply on [reload](/glossary/index.md#reload-from-disk).

## `dataplane`

| Property | Value |
|----------|--------|
| **Type** | Mapping (object) |
| **Required** | No — defaults apply when the block is omitted |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

When **`dataplane:`** is omitted, Conduit runs the **`sync`** runtime with the defaults in the table below.

## Block fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `runtime` | string | no | **`sync`** | Dataplane execution model: `sync` or `split_io`. See [Runtime values](#runtime-values). |
| `policy_workers` | integer | no | **1** | Number of policy worker threads under **`split_io`**. Must be ≥ 1 when `runtime: split_io`; **ignored** under `sync`. |
| `io_workers` | integer | no | **1** | Number of I/O worker threads under **`split_io`**. Must be ≥ 1 when `runtime: split_io`; **ignored** under `sync`. |
| `slot_chunk_size` | integer | no | **256** | Growth chunk (in [transaction](/glossary/index.md#transaction) slots) for the slot pool. Applies to both runtimes; must be ≥ 1 when set. See [Slot chunk size](#slot-chunk-size). |

## Runtime values

| Value | Status | Behavior |
|-------|--------|----------|
| **`sync`** (default) | Shipped | One ingress worker runs the whole pipeline on its own thread, **including the blocking upstream wait**, then sends the reply before taking the next query. See [Sync runtime](/concepts/runtime-and-concurrency.md#sync-runtime-default). |
| **`split_io`** | Shipped | Separate **ingress**, **policy**, and **I/O** worker pools; upstream waits are parked so ingress keeps accepting queries during slow upstreams. See [Split I/O runtime](/concepts/runtime-and-concurrency.md#split-io-runtime). |

Any other value is rejected by **`conduitctl validate --file`**. Use `sync` or `split_io`.

## Worker pools (`split_io`)

**`policy_workers`** and **`io_workers`** size the two non-ingress pools that `split_io` adds:

- **`policy_workers`** — threads that run the orchestrator phases ([Request rules](/concepts/architecture-and-packet-path.md#request-rules), [Lookup](/concepts/architecture-and-packet-path.md#lookup) including forward-provider submit) and finish each [transaction](/glossary/index.md#transaction) at [Response rules](/concepts/architecture-and-packet-path.md#response-rules) / [Send](/concepts/architecture-and-packet-path.md#send). Raise it for more concurrent policy and [Rhai](/rhai/index.md) execution.
- **`io_workers`** — threads that own the upstream sockets, match replies to parked transactions, and enforce `forward.timeout_ms`. Raise it for more upstream socket fan-out.

Both default to **1** and are **ignored under `sync`** (which has no separate policy/I/O pools — the ingress thread does everything). Ingress thread count is **not** set here: it comes from **`listeners.threads`** (per listener). See [Worker counts and limits](/concepts/runtime-and-concurrency.md#worker-counts-and-limits) and [Listeners](/reference/config-schema/listeners.md).

## Slot chunk size

Both runtimes track in-flight queries in a preallocated [transaction](/glossary/index.md#transaction) **slot pool** that grows in chunks up to **`orchestrator.txn_table_capacity`** (default **1024**). **`slot_chunk_size`** sets how many slots each growth step allocates (default **256**) — a tuning knob that trades startup/steady-state memory against allocation frequency under rising load. It does **not** change the pool's ceiling; that is **`orchestrator.txn_table_capacity`** ([Orchestrator](/reference/config-schema/orchestrator.md)). See [Transaction slot pool](/concepts/runtime-and-concurrency.md#transaction-slot-pool).

## Reload and restart

The `dataplane:` block is a **start-time** setting.

| Change | Effect |
|--------|--------|
| Edit **`dataplane:`** on disk + [reload](/glossary/index.md#reload-from-disk) (SIGHUP or **`conduitctl reload`**), or **`conduitctl apply`** a patch | The [runtime snapshot](/glossary/index.md#runtime-snapshot) updates, but the running runtime model and worker counts do **not** change |
| **Restart** | **Required** for a new `runtime`, `policy_workers`, `io_workers`, or `slot_chunk_size` to take effect |

This differs from dynamic blocks such as [`shutdown:`](/reference/config-schema/shutdown.md), which are read live. See [Architecture — Runtime snapshot](/concepts/architecture-and-packet-path.md#runtime-snapshot) for which changes need a restart.

## Validation summary

**`conduitctl validate --file …`** rejects:

- `runtime` other than `sync` or `split_io`.
- `policy_workers: 0` or `io_workers: 0` when `runtime` is `split_io`.
- `slot_chunk_size: 0` when the field is set.

Under `sync`, `policy_workers` and `io_workers` are not validated for range because they are unused.

## Example configuration

Default — omit the block (equivalent to `runtime: sync`):

```yaml
# no dataplane: block — sync runtime
listeners:
  threads: 2
```

Split I/O runtime with four policy workers and one I/O worker:

```yaml
dataplane:
  runtime: split_io
  policy_workers: 4
  io_workers: 1
listeners:
  threads: 2
  reuse_port: true
```

Larger slot-pool growth chunk for a high-capacity deployment:

```yaml
dataplane:
  runtime: split_io
  policy_workers: 8
  io_workers: 2
  slot_chunk_size: 1024
orchestrator:
  txn_table_capacity: 65536
```

## Related topics

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — runtime models, worker roles, slot pool, and drain
- [Listeners](/reference/config-schema/listeners.md) — `threads`, `reuse_port` (ingress workers)
- [Orchestrator](/reference/config-schema/orchestrator.md) — `txn_table_capacity` (slot-pool ceiling)
- [Pools](/reference/config-schema/pools.md) — `max_inflight` per-pool concurrency cap
- [Forward](/reference/config-schema/forward.md) — `outstanding_per_backend`, `timeout_ms`
- [Configuration model](/control-plane/configuration-model.md) — snapshots, reload, and what needs a restart
- [Config schema overview](/reference/config-schema/index.md)
