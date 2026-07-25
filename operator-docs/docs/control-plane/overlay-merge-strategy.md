# Overlay merge strategy

How Conduit merges an [overlay](/glossary/index.md#overlay) patch into the [file layer](/glossary/index.md#file-layer). Most top-level sections use **section replace**; a few surfaces use **deep merge** with documented list and match-by-name rules.

Background: [Configuration model](/control-plane/configuration-model.md). Apply workflows: [Reload and export](/control-plane/reload-and-export.md).

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

## Choosing a strategy as an operator

- Prefer **sparse overlays** that only set the keys you intend to change.
- For **section-replace** topics, include the full section you want effective (missing nested keys are not “keep file” — the overlay section replaces the file section).
- For **`metrics`**, nested maps keep file-layer siblings; lists under `categories` replace only when you send that list key.

## Related topics

- [Configuration model](/control-plane/configuration-model.md)
- [Metrics configurability](/observability/metrics-configurability.md)
- [Reload and export](/control-plane/reload-and-export.md)
- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)
