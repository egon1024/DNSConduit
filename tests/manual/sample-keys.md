# Manual test: `sample_percent` keys

Config: [`config/10-sample-keys.yaml`](config/10-sample-keys.yaml)

Fixture reference: [`../fixtures/config/with-sample-key.yaml`](../fixtures/config/with-sample-key.yaml)

## Validate

```bash
cargo build -p conduitctl
./target/debug/conduitctl validate --file tests/manual/config/10-sample-keys.yaml
./target/debug/conduitctl validate --file tests/fixtures/config/with-sample-key.yaml
```

Expect both to pass.

## Reject invalid combinations

```bash
# key and key_from together on a selector should fail validate
```

Edit a copy locally: add both `key` and `key_from` on one `sample_percent` selector; `conduitctl validate` should report they are mutually exclusive.

## Runtime spot-check (optional)

1. Start Conduit with `10-sample-keys.yaml` and a mock backend.
2. Send repeated queries for `foo.audit.example.` — only ~25% should get `audit_sampled` (stable per transaction id and key `audit.example`).
3. Repeat the same query from the same client path — the same transaction should always get the same pass/fail for that key.
