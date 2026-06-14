# mTLS

When **`control.tls`** is set, Conduit serves the control plane over **TLS**. If **`client_ca_path`** is also set, the server **requires a client certificate** signed by that CA (mutual TLS).

## Configuring TLS

```yaml
control:
  listen_address: "0.0.0.0:5199"
  tls:
    cert_path: tls/server.pem
    key_path: tls/server-key.pem
    client_ca_path: tls/ca.pem   # omit for TLS without client certs
```

Paths are [config-relative](/control-plane/config-file.md#path-resolution-base-directory) unless absolute.

## Clients

- Use **`https://`** in **`CONDUIT_CONTROL`** / **`conduitctl --endpoint`** when the server uses TLS.
- Configure your gRPC client with the server CA and, when required, a client certificate and key matching **`client_ca_path`**.
- **`conduitctl`** today connects with tonic’s default TLS roots for HTTPS endpoints; mTLS from the CLI may require additional client certificate configuration not yet built into `conduitctl` — use a gRPC client library or proxy where mTLS is required.

API keys (`control.api_keys`) apply independently: when keys are configured, callers still send **`Authorization: Bearer …`** even over mTLS.

## Related topics

- [Reference: control](/reference/config-schema/control.md)
- [API keys](/security/api-keys.md)
- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)
