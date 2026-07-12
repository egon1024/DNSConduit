# Reference: config schema

This is the field-level reference for Conduit's YAML config (`schema_version: 1`). Use the pages in this section when you need exact keys, types, and defaults — each nav entry matches a top-level block (or a nested path such as **`pools[].health`**). Pages are ordered like a query path: ingress, policy, answer path, upstream, process, control, then observability. **`metrics`** and **`tracing`** share one page; **`data_sources`** and **`data_source_limits`** share another.

For how the file is loaded, how paths resolve, and how validation behaves, see [Config file](/control-plane/config-file.md). For overlays, effective config, and snapshots, see [Configuration model](/control-plane/configuration-model.md). For how all top-level blocks fit together, see [Config file — top-level blocks](/control-plane/config-file.md#top-level-blocks).

The canonical machine-readable schema is `proto/conduit/v1/config.proto` in the repository.
