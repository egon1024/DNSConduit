# Transaction API

Rule Rhai scripts receive a `txn` object. This page documents the sampling helper for policy gating.

## `txn.sample_percent(percent)` / `txn.sample_percent(percent, key)`

- **Hook:** request and response
- **Arguments:** float `percent` in `0..100`; optional string `key` (salt)
- **Behavior:** deterministic sampling — same transaction id and key always get the same pass/fail. Without `key`, uses the global bucket (same as a YAML `sample_percent` selector with no `key`). With `key`, uses the same keyed hash as YAML `key: "…"`.
- **Side effect:** sets tag `sampled=true` when the call returns `true` (any successful sample call in the script run)
- **Caching:** one result per `(percent, key)` pair per script execution

Examples:

```rhai
// ~5% of all transactions (global bucket)
if txn.sample_percent(5.0) {
    txn.set_tag("audit", true);
}

// ~10% per qname (dynamic salt)
if txn.sample_percent(10.0, question_qname()) {
    txn.set_tag("per_name_audit", true);
}

// static policy salt (matches YAML key: "vip-zone")
if txn.sample_percent(25.0, "vip-zone") {
    txn.set_tag("vip_sampled", true);
}
```

See [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence) for how keys interact with `qname_suffix` and other selectors.
