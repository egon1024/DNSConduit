# Control plane

How Conduit loads configuration, how you change it on a running process, and how **`conduitctl`** talks to the optional gRPC API.

**Read in order:**

1. [Configuration model](/control-plane/configuration-model.md) — [file layer](/glossary/index.md#file-layer), [overlay](/glossary/index.md#overlay), [effective config](/glossary/index.md#effective-config), and [runtime snapshot](/glossary/index.md#runtime-snapshot)
2. [Config file](/control-plane/config-file.md) — YAML on disk, path resolution, validation
3. [Reload and export](/control-plane/reload-and-export.md) — **when** to reload, apply, or export; apply modes; common workflows
4. [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md) — **how** to connect, authenticate, and run each CLI command or RPC

**Reference (field lists and proto detail):**

- [Reference: control](/reference/config-schema/control.md) — `control:` block in config
- [Reference: gRPC and CLI](/reference/grpc-and-cli.md) — RPC messages and enum values

**Related:** [Security — API keys](/security/api-keys.md), [Security — mTLS](/security/mtls.md) when the control listener uses TLS.
