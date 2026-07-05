# Manual testing assets (DNSConduit product repo)

Manual test **procedures** live in **DNSConduitCursor** — see [docs/superpowers/process/manual-testing.md](../../docs/superpowers/process/manual-testing.md) and the lab index below.

This directory holds **assets only**: configs (config/), scripts (scripts/), and data files. Run lab commands from **~/git_repos/DNSConduit** in **zsh**. Upstream resolvers for dnsmasq: **192.168.1.21** (primary, port 15300) and **192.168.1.25** (secondary, port 15399). Start dnsmasq with **`-d`** so **Ctrl-C** stops it — see [manual-testing.md](../../docs/superpowers/process/manual-testing.md#dnsmasq-invocation-interactive-labs).

## Lab procedures (DNSConduitCursor)

| Topic | Procedure |
|-------|-----------|
| Lab ports and authoring rules | [manual-testing.md](../../docs/superpowers/process/manual-testing.md) |
| IPv4/IPv6 forwarding | [manual-lab-ipv4-ipv6-forwarding.md](../../docs/superpowers/plans/manual-lab-ipv4-ipv6-forwarding.md) |
| Backend health | [manual-lab-backend-health.md](../../docs/superpowers/plans/manual-lab-backend-health.md) |
| Dataplane runtime (sync / split_io) | [manual-lab-dataplane-runtime.md](../../docs/superpowers/plans/manual-lab-dataplane-runtime.md) |
| Metrics and tracing | [manual-lab-metrics-tracing.md](../../docs/superpowers/plans/manual-lab-metrics-tracing.md) |
| Operator metrics (4b) | [manual-lab-operator-metrics.md](../../docs/superpowers/plans/manual-lab-operator-metrics.md) |
| Control plane / overlay | [manual-lab-control-plane.md](../../docs/superpowers/plans/manual-lab-control-plane.md) |
| Rhai runtime host API | [manual-lab-rhai-runtime-host-api.md](../../docs/superpowers/plans/manual-lab-rhai-runtime-host-api.md) |
| Ordered rule actions | [manual-lab-ordered-rule-actions.md](../../docs/superpowers/plans/manual-lab-ordered-rule-actions.md) |
| Rhai sample keys | [manual-lab-rhai-sample-keys.md](../../docs/superpowers/plans/manual-lab-rhai-sample-keys.md) |
| Pluggable lookup checklist | [manual-lab-pluggable-lookup-checklist.md](../../docs/superpowers/plans/manual-lab-pluggable-lookup-checklist.md) |
| Pluggable lookup Gate B | [2026-07-05-pluggable-lookup-gate-b-manual-lab.md](../../docs/superpowers/plans/2026-07-05-pluggable-lookup-gate-b-manual-lab.md) |
| Release workflow validation | [release-workflow-manual-testing.md](../../docs/superpowers/process/release-workflow-manual-testing.md) |
| Release artifact validation | [release-artifacts-manual-testing.md](../../docs/superpowers/process/release-artifacts-manual-testing.md) |

## Pre-flight

```zsh
cd ~/git_repos/DNSConduit
export UPSTREAM_DNS_PRIMARY=192.168.1.21
export UPSTREAM_DNS_SECONDARY=192.168.1.25
chmod +x tests/manual/scripts/check-ports.sh
tests/manual/scripts/check-ports.sh
```

## Configs

Lab YAML configs: [tests/manual/config/](config/) (.yml only). Naming and port conventions are defined in [manual-testing.md](../../docs/superpowers/process/manual-testing.md).
