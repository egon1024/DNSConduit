# Config schema: acls

This page lists the fields for optional top-level **`acls:`** and per-listener **`acls:`**. For when to use ACLs and how they run on ingress, see [Client ACLs](/policy-routing/client-acls.md).

## `acls` (top-level)

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, every client is admitted |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) or [overlay](/glossary/index.md#overlay) (whole-section replace when present) |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `default_action` | string | no | **`allow`** | **`allow`** or **`deny`**. Applied when no rule matches. **`deny`** = silent drop (no DNS reply). |
| `rules` | list | no | `[]` | Ordered ACL rules; first match wins |

### Rule object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `match` | string | yes | Name of a **`type: cidr`** entry under **`data_sources:`** |
| `action` | string | yes | **`drop`**, **`refuse`**, **`tag`**, or **`accept`** |
| `tag` | string | when `action: tag` | Single tag name to set on the transaction |

```yaml
acls:
  default_action: deny
  rules:
    - match: block_nets
      action: drop
    - match: corp_nets
      action: accept
    - match: partners
      action: tag
      tag: partner
```

## Per-listener `acls`

Same shape as top-level **`acls:`**, nested on a [listener object](/reference/config-schema/listeners.md#listener-object).

| When | Effect |
|------|--------|
| Omitted | Inherit top-level **`acls:`** entirely (or admit-all) |
| Set | **Full replace** for that listener — not a merge with global |

## Validation summary

| Rule | Typical failure |
|------|-----------------|
| `match` names a `type: cidr` source | Unknown or non-cidr data source name |
| `action: tag` includes `tag` | Missing tag name |
| Known `action` / `default_action` | Unknown action string |

## Reload and overlay

| Change | How it applies |
|--------|----------------|
| Top-level **`acls:`** | Hot via reload / overlay replace into the [runtime snapshot](/glossary/index.md#runtime-snapshot) |
| **`data_sources:`** (CIDR files) | Hot on snapshot rebuild (file re-read) |
| Per-listener **`acls:`** | File reload, or whole-**`listeners:`** overlay replace |

Export preserves structure and paths, not prefix file contents. See [Client ACLs — Overlay and export](/policy-routing/client-acls.md#overlay-and-export).

## Related topics

- [Client ACLs](/policy-routing/client-acls.md)
- [Config schema: data sources](/reference/config-schema/data-sources.md)
- [Config schema: listeners](/reference/config-schema/listeners.md)
- [Configuration model](/control-plane/configuration-model.md)
