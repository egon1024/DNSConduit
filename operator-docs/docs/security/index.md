# Security

This section covers hardening the optional [control plane](/glossary/index.md#control-plane) — API keys and TLS for gRPC. Field lists for **`control:`** are in [Reference: control](/reference/config-schema/control.md). DNS [dataplane](/glossary/index.md#dataplane) listeners are separate; see [Reference: listeners](/reference/config-schema/listeners.md).

- [API keys](/security/api-keys.md) — `control.api_keys` and `conduitctl` authentication
- [mTLS](/security/mtls.md) — `control.tls` and client certificate requirements
