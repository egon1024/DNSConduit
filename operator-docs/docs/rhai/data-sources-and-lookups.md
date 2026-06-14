# Data sources and lookups

<!-- Maintainer: IA revisit when WASM or sidecar ship — data_sources is tier-agnostic; consider moving primary home out of Rhai nav. -->
<!-- Maintainer: When writing this page — document CSV in-memory load today; planned lookup backend augmentation and refresh behavior. -->

Lookup tables declared under **`data_sources:`** in config — host-owned data for **`table_lookup`** in [Rhai](/rhai/index.md) scripts. Overview and current-release behavior (CSV loaded into memory at snapshot build, planned backend expansion): [Lookup tables](/concepts/extensibility.md#lookup-tables). Config paths: [Config file](/control-plane/config-file.md#path-resolution-base-directory).
