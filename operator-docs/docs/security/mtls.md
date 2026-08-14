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

- Use **`https://`** in **`CONDUIT_CONTROL`** / **`conduitctl --endpoint`** (or the client config `endpoint`) when the server uses TLS.
- Trust the server with **`--tls-ca`** / `CONDUIT_TLS_CA` / client file **`tls.ca`**, or rely on the client’s normal trusted roots when the cert chains there.
- For mTLS, present **`--tls-cert`** and **`--tls-key`** (or env / client file **`tls.cert`** / **`tls.key`**) matching **`client_ca_path`**.
- Chain and hostname verification are **on by default**. Use **`--tls-insecure`** only as an explicit opt-out (for example a self-signed server cert without distributing a CA).

Full flag, env, and YAML client-file reference: [gRPC and conduitctl — Connecting](/control-plane/grpc-and-conduitctl.md#connecting).

API keys (`control.api_keys`) apply independently: when keys are configured, callers still send **`Authorization: Bearer …`** even over mTLS.

## Related topics

- [Reference: control](/reference/config-schema/control.md)
- [API keys](/security/api-keys.md)
- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)
