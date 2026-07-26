# Control plane

This section covers how Conduit loads configuration, how you change it on a running process, and how **`conduitctl`** talks to the optional gRPC API.

**Read in order:**

1. [Configuration model](/control-plane/configuration-model.md) — [file layer](/glossary/index.md#file-layer), [overlay](/glossary/index.md#overlay), [effective config](/glossary/index.md#effective-config), and [runtime snapshot](/glossary/index.md#runtime-snapshot)
2. [Overlay merge strategy](/control-plane/overlay-merge-strategy.md) — section replace vs deep merge (`metrics`, pools)
3. [Config file](/control-plane/config-file.md) — YAML on disk, path resolution, validation
4. [Reload and export](/control-plane/reload-and-export.md) — **when** to reload, apply, or export; apply modes; common workflows
5. [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md) — **how** to connect, authenticate, and run each CLI command or RPC (`apply`, `export`, `reload`, `trace`, **`health`**, …)

**Reference (field lists and proto detail):**

- [Reference: control](/reference/config-schema/control.md) — `control:` block in config
- [Reference: gRPC and CLI](/reference/grpc-and-cli.md) — RPC messages and enum values

**Related:** [Backend health](/policy-routing/backend-health.md) and [`conduitctl health`](/control-plane/grpc-and-conduitctl.md#health) for per-backend freeze/drain; [Security — API keys](/security/api-keys.md), [Security — mTLS](/security/mtls.md) when the control listener uses TLS.
