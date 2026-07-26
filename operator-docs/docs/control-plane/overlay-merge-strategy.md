# Overlay merge strategy

Conduit merges a configuration [overlay](/glossary/index.md#overlay) into the on-disk [file layer](/glossary/index.md#file-layer) using **section replace** for most top-level configuration keys; a few surfaces use **deep merge** with documented list and match-by-name rules. For how layers fit together, see [Configuration model](/control-plane/configuration-model.md); for apply and reload steps, see [Reload and export](/control-plane/reload-and-export.md).

## Section replace (default)

If the overlay includes a top-level key, that **entire section** from the file layer is replaced by the overlay's section (scalars and nested structure as present in the patch).

Examples: **`listeners`**, **`forward`**, **`orchestrator`**, **`events`**, **`rhai`**, **`control`**, **`logging`**. A non-empty overlay **`data_sources`** list replaces the file-layer list.

Sparse patches that omit a top-level key leave that file-layer section unchanged.

## Deep merge surfaces

| Surface | Strategy | Summary |
|---------|----------|---------|
| **`metrics`** | Deep merge | Nested maps merge by key; see [Metrics deep merge](#metrics-deep-merge) |
| **`pools`** | Match-by-name (related pattern) | Pools by `name`; backends by `name` or `address` — see [Configuration model — pools](/control-plane/configuration-model.md#how-file-and-overlay-merge) |

**`rules`** and **`tracing`** remain **file-layer only** — overlays that include those keys are rejected.

### Metrics deep merge

When the overlay includes **`metrics:`**:

| Field | Merge rule |
|-------|------------|
| Scalars (`enabled`, `base`, `profile`, …) | Overlay wins when set / non-empty |
| Nested maps (`collection`, `granularity.overrides`, OTEL attrs/headers, …) | Deep-merge by key; overlay values win per key |
| `categories.include` / `categories.exclude` | **List replace** when that key is present in the patch |
| `user_metrics` | **Match-by-name**: update matching entries; append new names |
| `prometheus` / `otel` | Nested deep merge (address/path/endpoint and related fields) |

Plan knobs (base, categories, collection, granularity, user metrics, event_export) apply on snapshot swap without restart. Changing Prometheus listen settings hot-rebinds; bind failure rejects the apply. Details: [Metrics configurability — Overlay and live apply](/observability/metrics-configurability.md#overlay-and-live-apply).

Example — exclude timing without rewriting the whole metrics block:

```yaml
schema_version: 1
metrics:
  categories:
    exclude: [timing]
```

```bash
conduitctl apply --file metrics-exclude-timing.yaml
```

## Examples

### A complete overlay document

An overlay is ordinary Conduit YAML with **`schema_version`** and only the top-level keys you intend to change. Omitted keys leave the [file layer](/glossary/index.md#file-layer) alone. Here a single apply lowers one backend’s weight and raises orchestrator retry budget:

```yaml
schema_version: 1
pools:
  - name: default
    backends:
      - address: "10.0.0.1:53"
        weight: 10
orchestrator:
  max_attempts: 5
  max_txn_duration_ms: 8000
  txn_table_capacity: 2048
```

```bash
conduitctl apply --file maintenance-overlay.yaml
```

**`pools`** uses match-by-name (only the listed backend fields update). **`orchestrator`** is **section replace** — the overlay must carry every orchestrator field you want to keep from the file layer (see below). Confirm with **`conduitctl export`**. Drop the overlay later with **`conduitctl apply --clear`** or [reload from disk](/glossary/index.md#reload-from-disk).

### Unintentional clobber (section replace)

Suppose the on-disk file already tunes more than retry count:

```yaml
# fragment of the file layer
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 8000
  txn_table_capacity: 2048
```

An operator who only wants a higher retry count might apply:

```yaml
schema_version: 1
orchestrator:
  max_attempts: 5
```

Because **`orchestrator`** is section replace, that patch becomes the **entire** effective orchestrator block. **`max_txn_duration_ms`** and **`txn_table_capacity`** fall back to built-in defaults (**5000** and **1024**), not the file-layer values — even though those keys were never mentioned in the patch.

The same trap applies to other section-replace surfaces (`listeners`, `forward`, `events`, `rhai`, `control`, `logging`, and a non-empty **`data_sources`** list). Sparse patches are safe only when omitting a **top-level** key; within a replaced section, omitted nested keys are not “keep file.”

### Augment a section safely (export, mutate, apply)

For section-replace topics, treat the overlay section as a full replacement: start from what is already effective, change only what you need, and send the whole section back.

**1 — Export** the running [effective config](/glossary/index.md#effective-config):

```bash
conduitctl export --output /tmp/conduit-effective.yaml
```

**2 — Copy** the section you will change (here **`orchestrator:`**) into a new patch file. Keep **`schema_version: 1`**. Leave every other top-level key out of the patch so those file-layer sections stay untouched.

**3 — Edit** only the fields you intend to change, leaving the rest of the section as exported:

```yaml
schema_version: 1
orchestrator:
  max_attempts: 5          # raised
  max_txn_duration_ms: 8000
  txn_table_capacity: 2048
```

**4 — Apply** and verify:

```bash
conduitctl apply --file orchestrator-augmented.yaml
conduitctl export | grep -A5 '^orchestrator:'
```

Export may omit fields that equal built-in defaults — that is normal normalization, not a missing setting. See [Reload and export — export](/control-plane/reload-and-export.md#export-effective-configuration).

**`metrics`** is the usual exception: deep merge lets you send a sparse nested patch (as in [Metrics deep merge](#metrics-deep-merge)) without rewriting sibling maps. **`pools`** patches update matched pools/backends without replacing the whole list.

## Choosing a strategy as an operator

- Prefer **sparse overlays** that only set the **top-level** keys you intend to change.
- For **section-replace** topics, include the **full section** you want effective — use [export, mutate, apply](#augment-a-section-safely-export-mutate-apply) when augmenting an existing section. Missing nested keys are not “keep file.”
- For **`metrics`**, nested maps keep file-layer siblings; lists under `categories` replace only when you send that list key.
- For **`pools`**, match-by-name patches can stay sparse at the backend field level; see [Configuration model — pools](/control-plane/configuration-model.md#how-file-and-overlay-merge).

## Related topics

- [Configuration model](/control-plane/configuration-model.md)
- [Metrics configurability](/observability/metrics-configurability.md)
- [Reload and export](/control-plane/reload-and-export.md)
- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)
