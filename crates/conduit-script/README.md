# conduit-script

Rhai scripting for Conduit rule hooks.

## Transaction API (summary)

| Method | Request hook | Response hook |
|--------|--------------|---------------|
| `set_pool(name)` | Yes | Yes |
| `set_retry_pool(name)` | Pool for retry Route if retry occurs; first Route ignores | Pool for retry Route if retry occurs; first Route ignores |
| `request_retry()` | No effect | Soft retry |
| `request_retry_now()` | No effect | Hard retry (stop rule) |
| `clear_retry()` | No effect | Clear soft retry |
| `clear_retry_pool()` | Clears `retry_pool` | Clears `retry_pool` |
| `drop_query()` | Soft drop | Soft drop |
| `drop_query_now()` | Hard drop | Hard drop |
| `clear_drop()` | Yes | Yes |

See operator docs in `operator-docs/docs/rhai/hooks-and-phases.md`.
