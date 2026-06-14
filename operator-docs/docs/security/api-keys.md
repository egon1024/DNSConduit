# API keys

When **`control.api_keys`** lists one or more secrets, every gRPC control RPC requires a matching key:

- **`Authorization: Bearer YOUR_KEY`** (preferred — use `--api-key` / `CONDUIT_API_KEY` with `conduitctl`)
- **`x-api-key: YOUR_KEY`** (also accepted by the server)

When **`api_keys`** is empty or omitted, control RPCs accept anonymous callers on the listen address — suitable only on trusted networks (for example `127.0.0.1`).

## Configuring keys

```yaml
control:
  listen_address: "127.0.0.1:5199"
  api_keys:
    - "replace-with-a-long-random-secret"
```

Keys are compared literally against the **active snapshot** — a successful **`conduitctl apply`** or reload that changes `api_keys` affects the **next** RPC without restarting the process.

## Using keys with conduitctl

```bash
export CONDUIT_API_KEY='replace-with-a-long-random-secret'
conduitctl reload

# or per invocation
conduitctl --api-key 'replace-with-a-long-random-secret' export
```

Invalid or missing keys return **Unauthenticated**; the server logs **`control rpc`** with requestor **`api_key_rejected`** or **`unauthenticated`** without logging the key value.

## Related topics

- [Reference: control](/reference/config-schema/control.md)
- [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)
- [mTLS](/security/mtls.md)
