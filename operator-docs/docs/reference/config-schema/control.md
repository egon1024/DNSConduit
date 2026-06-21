# Reference: control

The optional **`control:`** block enables the gRPC [control plane](/glossary/index.md#control-plane) and **`conduitctl`**. Omit **`control:`** entirely when you only need DNS service and reload via **SIGHUP** or process restart.

The listener is created from the config present at **process start**. Reloading a new `control:` section into the snapshot does not start gRPC until you restart `conduit`.

| Field {: .column-no-wrap } | Type | Default | Purpose |
|-------|------|---------|---------|
| `listen_address` | string | (required when block present) | gRPC bind address, e.g. `127.0.0.1:5199` |
| `reflection_enabled` | bool | `false` | Register gRPC server reflection (dev/test) |
| `api_keys` | list of strings | `[]` | When non-empty, require `Authorization: Bearer <key>` on control RPCs |
| `tls` | object | omitted | When set, serve gRPC over TLS; see below |

### `tls`

Paths resolve relative to the [config file directory](/control-plane/config-file.md#path-resolution-base-directory) (same as Rhai scripts and event sinks).

| Field | Purpose |
|-------|---------|
| `cert_path` | Server certificate PEM |
| `key_path` | Server private key PEM |
| `client_ca_path` | When non-empty, require client certificates (mTLS) |

Operator setup: [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md), [API keys](/security/api-keys.md), [mTLS](/security/mtls.md).

## Related topics

- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)
- [Reference: gRPC and CLI](/reference/grpc-and-cli.md)
- [Config file](/control-plane/config-file.md)
