# Transaction API

Rule Rhai scripts receive a `txn` object. This page currently documents the sampling helper added for policy gating.

## `txn.sample_percent(percent)`

- **Hook:** request and response
- **Argument:** float in `0..100`
- **Behavior:** deterministic sampling per transaction id; returns `true` when this transaction is included at the requested percentage
- **Side effect:** sets tag `sampled=true` when the call returns `true`

Example:

```rhai
if txn.sample_percent(5.0) {
    txn.set_tag("audit", true);
}
```
