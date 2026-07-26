---
toc_depth: 3
---

# Lookups

**`lookup(table, key)`** and **`lookup_ip(name, addr)`** are global functions in [Rhai for rules](/rhai/rule-rhai.md) that read named tables and views declared under **`data_sources:`** — exact-key CSV with **`lookup`**, longest-prefix CIDR with **`lookup_ip`**. The config, file formats, load-safety limits, and reload behavior live on [Data sources](/policy-routing/data-sources.md); this page covers the Rhai calling surface and patterns.

Both are global functions — **not** methods on **`txn`** — and they read **`data_sources:`** policy tables, **not** the [Lookup](/concepts/architecture-and-packet-path.md#lookup) pipeline phase. See [Glossary — Lookup vs lookup(table, key)](/glossary/index.md#lookup-vs-lookuptable-key). Which hooks may call them is covered under [Hooks and phases — phase guards](/rhai/hooks-and-phases.md#phase-guards).

## Overview

| Function | Returns |
|----------|---------|
| **`lookup(table, key)`** | Exact-key CSV — the value string, or **`""`** on miss |
| **`lookup_ip(name, addr)`** | Longest-prefix CIDR — the value string, or **`""`** on miss |

Only names listed under **`data_sources:`** are visible to scripts — see the [grant model](/policy-routing/data-sources.md#overview). Both calls count toward [sandbox limits](/rhai/sandbox-limits.md) (`rhai.max_operations`, `rhai.hook_timeout_ms`) like other host calls.

## `lookup` behavior { #lookup-behavior }

```rhai
let action = lookup("blocklist", txn.question().qname);
```

| Situation | Return value | Observability |
|-----------|----------------|---------------|
| Key found | Value cell as string | — |
| Key not in table | `""` | Silent (expected miss) |
| Unknown `table` name (not in `data_sources:`) | `""` | Warn log (milestone + periodic) and [`conduit_script_errors_total`](/observability/built-in-metrics.md#conduit_script_errors_total) (`reason="lookup_unknown_table"`) |
| Empty value cell | `""` (still a “hit” if key exists — rare in practice) | — |

**Compile-time check:** when the table argument is a **string literal** in Rhai source (for example `lookup("blocklist", …)`), Conduit validates the name against **`data_sources:`** at snapshot build. A typo fails **`conduitctl validate`** and reload. Dynamic table names (variable or expression) are not checked at compile time — they surface at runtime with the observability row above.

Use **`txn.question().qname`** for qname-keyed tables. On the [response hook](/rhai/hooks-and-phases.md#response-hook), the question is unchanged from the client query.

## `lookup_ip` behavior { #lookup-ip-behavior }

```rhai
// Hit → non-empty string (value or membership marker); miss → ""
if lookup_ip("corp_nets", txn.client_ip()) != "" {
    txn.set_tag("corp", true);
}
```

**`lookup_ip`** does a longest-prefix match over a named **`type: cidr`** view: the **most-specific** matching prefix wins, a hit returns a non-empty string (the trailing value, or a membership marker when the line has none), and a miss returns **`""`**. IPv4 and IPv6 are both first-class. File format and file-side semantics: [Data sources — CIDR sources](/policy-routing/data-sources.md#cidr-sources).

The same **`type: cidr`** views back host [Client ACLs](/policy-routing/client-acls.md); use **`lookup_ip`** when you want the membership decision inside a rule script instead of the host ACL gate.

## Patterns

### Blocklist (request hook)

Script `blocklist.rhai`:

```rhai
if lookup("blocklist", txn.question().qname) == "block" {
    txn.drop_query_now();
}
```

Config: inline **`conduit.yaml`** beside the CSV — see [Rhai policy — Blocklist drop](/guides/rhai-policy.md#example-1-blocklist-drop-request-hook).

### Tag for observability { #tag-for-observability }

Script `lookup-demo.rhai`:

```rhai
let region = lookup("geo", txn.question().qname);
if region != "" {
    txn.set_tag("region", region);
}
```

Downstream sinks can filter on **`tag_required`** or rules can branch on **`txn.has_tag("region")`**. Walkthrough: [Event export and dnstap — Tag-gated export](/guides/event-export-dnstap.md#5-optional-checks).

### Pool or egress map { #pool-or-egress-map }

```rhai
let pool = lookup("routing", txn.question().qname);
if pool != "" {
    txn.set_pool(pool);
} else {
    txn.clear_pool();
}
```

List **`set_pool`** before **`rhai`** on the same rule when the script only refines pool choice and built-ins must run first — see [Action order on one rule](/policy-routing/rules-and-actions.md#action-order-on-one-rule).

## Runnable examples

| Script | In repository fixtures? | What it shows | Walkthrough |
|--------|-------------------------|----------------|-------------|
| `blocklist.rhai` | Yes (`tests/fixtures/rhai/`) | Drop on CSV `block` | [Rhai policy — Blocklist](/guides/rhai-policy.md#example-1-blocklist-drop-request-hook) |
| `route-by-table.rhai` | Guide only (copy from lab YAML) | CSV qname → pool | [Rhai policy — CSV pool routing](/guides/rhai-policy.md#example-2-csv-pool-routing-request-hook) |
| `lookup-demo.rhai` | Yes | Grant model — only configured tables | [Tag for observability](#tag-for-observability) |
| `block-hits.rhai` | Yes | Lookup + user metrics | [User metrics](/rhai/user-metrics.md) |

## Limitations (current release)

- Read-only from scripts — no write-back or per-query cache invalidation.
- [Rhai for rules](/rhai/rule-rhai.md) only — **`lookup`** and **`lookup_ip`** are available to rule scripts on the request and response hooks.
- Dynamic (non-literal) table names are validated at runtime, not at compile time.
- Source types and load caps: see [Data sources — Limitations](/policy-routing/data-sources.md#limitations-current-release).

## Related topics

- [Data sources](/policy-routing/data-sources.md) — `data_sources:` config, CSV / CIDR file formats, load-safety limits, reload
- [Host API overview](/rhai/host-api.md) — where lookups sit in the Rhai surface
- [Rhai for rules](/rhai/rule-rhai.md) — when to use scripts vs built-in selectors
- [Sandbox limits](/rhai/sandbox-limits.md) — operation and timeout caps
- [Event export](/observability/event-export.md) — tags set from lookup results
