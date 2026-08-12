# Unreleased

## Observability

- Control-plane **`control rpc`** access logs include **`tls=true|false`**, indicating whether the RPC arrived over TLS (transport encryption). This is separate from requestor **`mtls`** (client certificate identity).
- Failed control-plane connections that never become an RPC (TCP accept errors, TLS handshake failures) log at **`warn`** as **`control plane connection failed`** with **`tls`**, **`error`**, and **`peer`** when known.
