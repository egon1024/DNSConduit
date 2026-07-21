//! Client IP ACL evaluation: global/per-listener policy compile and
//! first-match evaluation over the shared CIDR store (client-acls decisions
//! 3-5).

use conduit_proto::config::AclsConfig;
use conduit_script::DataSourceStore;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclDefaultAction {
    Allow,
    Deny,
}

#[derive(Debug, Clone)]
pub enum AclAction {
    Drop,
    Refuse,
    Tag(String),
    Accept,
}

#[derive(Debug, Clone)]
pub struct CompiledAclRule {
    pub view: String,
    pub action: AclAction,
}

/// Compiled first-match ACL policy for a listener's effective `acls:` block.
#[derive(Debug, Clone)]
pub struct CompiledAclPolicy {
    pub default_action: AclDefaultAction,
    pub rules: Vec<CompiledAclRule>,
}

/// The result of evaluating an ACL policy for one client address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclDecision {
    /// No terminal ACL outcome; the query proceeds.
    Admit,
    /// Silent drop — no DNS response.
    Drop,
    /// Send REFUSED.
    Refuse,
    /// Admit and set the named tag on the transaction.
    Tag(String),
}

impl CompiledAclPolicy {
    /// No `acls:` anywhere for this listener: admit all clients.
    pub fn admit_all() -> Self {
        Self {
            default_action: AclDefaultAction::Allow,
            rules: Vec::new(),
        }
    }

    pub fn compile(cfg: &AclsConfig) -> Self {
        let default_action = match cfg.default_action.as_str() {
            "deny" => AclDefaultAction::Deny,
            _ => AclDefaultAction::Allow,
        };
        let rules = cfg
            .rules
            .iter()
            .map(|rule| CompiledAclRule {
                view: rule.r#match.clone(),
                action: match rule.action.as_str() {
                    "drop" => AclAction::Drop,
                    "refuse" => AclAction::Refuse,
                    "accept" => AclAction::Accept,
                    "tag" => AclAction::Tag(
                        rule.tag
                            .clone()
                            .expect("tag action must be validated before compile"),
                    ),
                    other => panic!("unknown ACL action '{other}' must be rejected at validate"),
                },
            })
            .collect();
        Self {
            default_action,
            rules,
        }
    }

    /// Resolves and compiles the effective policy: listener replace wins over
    /// global inherit; no `acls:` anywhere admits all clients.
    pub fn compile_effective(global: Option<&AclsConfig>, listener: Option<&AclsConfig>) -> Self {
        match effective_acl(global, listener) {
            Some(cfg) => Self::compile(cfg),
            None => Self::admit_all(),
        }
    }
}

/// Per-listener ACL replace semantics (spec: per-listener ACL replace
/// semantics): a listener's own `acls:` fully replaces global; omitted
/// listener `acls:` inherits global; both absent is `None` (admit-all).
pub fn effective_acl<'a>(
    global: Option<&'a AclsConfig>,
    listener: Option<&'a AclsConfig>,
) -> Option<&'a AclsConfig> {
    listener.or(global)
}

/// Tier 0 (pre-parse, pre-slot): only explicit `drop` matches are actionable
/// here. A first match on any other action defers to [`evaluate_full`] at
/// Tier 1 so first-match semantics stay consistent across tiers; there is no
/// default-deny short-circuit at this tier.
pub fn evaluate_preadmission(
    policy: &CompiledAclPolicy,
    addr: IpAddr,
    store: &DataSourceStore,
) -> AclDecision {
    for rule in &policy.rules {
        if store.lookup_ip(&rule.view, addr).is_some() {
            return match rule.action {
                AclAction::Drop => AclDecision::Drop,
                _ => AclDecision::Admit,
            };
        }
    }
    AclDecision::Admit
}

/// Tier 1 (post-parse, pre-slot): full first-match evaluation, including
/// `default_action` fall-through. `default_action: deny` resolves to a
/// silent [`AclDecision::Drop`], not REFUSED.
pub fn evaluate_full(
    policy: &CompiledAclPolicy,
    addr: IpAddr,
    store: &DataSourceStore,
) -> AclDecision {
    for rule in &policy.rules {
        if store.lookup_ip(&rule.view, addr).is_some() {
            return match &rule.action {
                AclAction::Drop => AclDecision::Drop,
                AclAction::Refuse => AclDecision::Refuse,
                AclAction::Tag(name) => AclDecision::Tag(name.clone()),
                AclAction::Accept => AclDecision::Admit,
            };
        }
    }
    match policy.default_action {
        AclDefaultAction::Allow => AclDecision::Admit,
        AclDefaultAction::Deny => AclDecision::Drop,
    }
}

/// Side-effect-free ACL check result for one listener (control plane / CLI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclCheckResult {
    pub listener: String,
    /// `admit`, `drop`, `refuse`, or `tag`.
    pub decision: String,
    /// Present when `decision` is `tag`.
    pub tag: Option<String>,
    /// Matching CIDR view name, or `"default"` when `default_action` applied.
    pub matched: String,
    /// Matched rule action (`drop`/`refuse`/`tag`/`accept`), or `allow`/`deny` for default.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclCheckError {
    /// `--listener` / filter did not match any configured listener name.
    UnknownListener(String),
    /// Config has no `listeners.listeners` entries.
    NoListeners,
}

impl std::fmt::Display for AclCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownListener(name) => write!(f, "unknown listener '{name}'"),
            Self::NoListeners => write!(f, "no listeners configured"),
        }
    }
}

impl std::error::Error for AclCheckError {}

/// Evaluate effective ACL policy for `addr` on each listener (or one filter).
///
/// Uses the same first-match / `default_action` semantics as Tier 1 ingress
/// (`evaluate_full`). Does **not** record metrics or emit denial logs.
pub fn check_client_acl(
    config: &conduit_proto::config::Config,
    store: &DataSourceStore,
    addr: IpAddr,
    listener_filter: Option<&str>,
) -> Result<Vec<AclCheckResult>, AclCheckError> {
    let Some(block) = config.listeners.as_ref() else {
        return Err(AclCheckError::NoListeners);
    };
    if block.listeners.is_empty() {
        return Err(AclCheckError::NoListeners);
    }

    let global = config.acls.as_ref();
    let mut results = Vec::new();
    let mut found_filter = false;

    for listener in &block.listeners {
        let name = conduit_config::resolve_listener_ingress(block, listener).name;
        if let Some(filter) = listener_filter {
            if name != filter {
                continue;
            }
            found_filter = true;
        }
        results.push(check_one_listener(
            global,
            listener.acls.as_ref(),
            &name,
            addr,
            store,
        ));
    }

    if let Some(filter) = listener_filter {
        if !found_filter {
            return Err(AclCheckError::UnknownListener(filter.to_string()));
        }
    }
    Ok(results)
}

fn check_one_listener(
    global: Option<&AclsConfig>,
    listener: Option<&AclsConfig>,
    listener_name: &str,
    addr: IpAddr,
    store: &DataSourceStore,
) -> AclCheckResult {
    let policy = CompiledAclPolicy::compile_effective(global, listener);
    let (matched, action) = match_detail(&policy, addr, store);
    let decision = evaluate_full(&policy, addr, store);
    let (decision_str, tag) = match decision {
        AclDecision::Admit => ("admit".to_string(), None),
        AclDecision::Drop => ("drop".to_string(), None),
        AclDecision::Refuse => ("refuse".to_string(), None),
        AclDecision::Tag(name) => ("tag".to_string(), Some(name)),
    };
    AclCheckResult {
        listener: listener_name.to_string(),
        decision: decision_str,
        tag,
        matched: matched.to_string(),
        action: action.to_string(),
    }
}

/// View name and rule/default action string that produced the Tier-1 outcome.
fn match_detail<'a>(
    policy: &'a CompiledAclPolicy,
    addr: IpAddr,
    store: &DataSourceStore,
) -> (&'a str, String) {
    for rule in &policy.rules {
        if store.lookup_ip(&rule.view, addr).is_some() {
            let action = match &rule.action {
                AclAction::Drop => "drop",
                AclAction::Refuse => "refuse",
                AclAction::Tag(_) => "tag",
                AclAction::Accept => "accept",
            };
            return (rule.view.as_str(), action.to_string());
        }
    }
    let action = match policy.default_action {
        AclDefaultAction::Allow => "allow",
        AclDefaultAction::Deny => "deny",
    };
    ("default", action.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::AclRule;

    fn store_with_cidr(name: &str, prefix: &str, value: &str) -> DataSourceStore {
        let dir = std::env::temp_dir().join(format!(
            "conduit-core-acl-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nets.txt");
        std::fs::write(&path, format!("{prefix} {value}\n")).unwrap();
        let yaml = format!(
            r#"schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
data_sources:
  - name: {name}
    type: cidr
    path: "{}"
"#,
            path.display()
        );
        let cfg = conduit_config::load_yaml(&yaml).unwrap();
        let scripting = conduit_script::compile_from_config(&cfg, None).unwrap();
        (*scripting.data_sources).clone()
    }

    fn accept_rule(view: &str) -> AclRule {
        AclRule {
            r#match: view.into(),
            action: "accept".into(),
            tag: None,
        }
    }

    fn drop_rule(view: &str) -> AclRule {
        AclRule {
            r#match: view.into(),
            action: "drop".into(),
            tag: None,
        }
    }

    #[test]
    fn effective_acl_listener_replaces_global() {
        let global = AclsConfig {
            default_action: "allow".into(),
            rules: vec![],
        };
        let listener = AclsConfig {
            default_action: "deny".into(),
            rules: vec![accept_rule("corp_nets")],
        };
        let effective = effective_acl(Some(&global), Some(&listener)).unwrap();
        assert_eq!(effective.default_action, "deny");
        assert_eq!(effective.rules.len(), 1);
    }

    #[test]
    fn effective_acl_omitted_listener_inherits_global() {
        let global = AclsConfig {
            default_action: "deny".into(),
            rules: vec![],
        };
        let effective = effective_acl(Some(&global), None).unwrap();
        assert_eq!(effective.default_action, "deny");
    }

    #[test]
    fn effective_acl_none_anywhere_is_admit_all() {
        assert!(effective_acl(None, None).is_none());
        let policy = CompiledAclPolicy::compile_effective(None, None);
        assert_eq!(policy.default_action, AclDefaultAction::Allow);
        assert!(policy.rules.is_empty());
    }

    #[test]
    fn accept_short_circuits_before_later_drop_rule() {
        let store = store_with_cidr("corp_nets", "10.0.0.0/8", "1");
        let cfg = AclsConfig {
            default_action: "allow".into(),
            rules: vec![accept_rule("corp_nets"), drop_rule("corp_nets")],
        };
        let policy = CompiledAclPolicy::compile(&cfg);
        let decision = evaluate_full(&policy, "10.1.2.3".parse().unwrap(), &store);
        assert_eq!(decision, AclDecision::Admit);
    }

    #[test]
    fn default_deny_is_silent_drop_on_no_match() {
        let store = store_with_cidr("corp_nets", "10.0.0.0/8", "1");
        let cfg = AclsConfig {
            default_action: "deny".into(),
            rules: vec![accept_rule("corp_nets")],
        };
        let policy = CompiledAclPolicy::compile(&cfg);
        let decision = evaluate_full(&policy, "192.0.2.1".parse().unwrap(), &store);
        assert_eq!(decision, AclDecision::Drop);
    }

    #[test]
    fn default_allow_admits_on_no_match() {
        let store = store_with_cidr("corp_nets", "10.0.0.0/8", "1");
        let cfg = AclsConfig {
            default_action: "allow".into(),
            rules: vec![drop_rule("corp_nets")],
        };
        let policy = CompiledAclPolicy::compile(&cfg);
        let decision = evaluate_full(&policy, "192.0.2.1".parse().unwrap(), &store);
        assert_eq!(decision, AclDecision::Admit);
    }

    #[test]
    fn preadmission_only_drops_explicit_drop_match() {
        let store = store_with_cidr("corp_nets", "10.0.0.0/8", "1");
        let cfg = AclsConfig {
            default_action: "deny".into(),
            rules: vec![drop_rule("corp_nets")],
        };
        let policy = CompiledAclPolicy::compile(&cfg);
        assert_eq!(
            evaluate_preadmission(&policy, "10.1.2.3".parse().unwrap(), &store),
            AclDecision::Drop
        );
        // No match: preadmission never applies default_action, even deny.
        assert_eq!(
            evaluate_preadmission(&policy, "192.0.2.1".parse().unwrap(), &store),
            AclDecision::Admit
        );
    }

    #[test]
    fn preadmission_defers_non_drop_first_match_to_full_tier() {
        let store = store_with_cidr("corp_nets", "10.0.0.0/8", "1");
        let cfg = AclsConfig {
            default_action: "allow".into(),
            rules: vec![
                AclRule {
                    r#match: "corp_nets".into(),
                    action: "refuse".into(),
                    tag: None,
                },
                drop_rule("corp_nets"),
            ],
        };
        let policy = CompiledAclPolicy::compile(&cfg);
        assert_eq!(
            evaluate_preadmission(&policy, "10.1.2.3".parse().unwrap(), &store),
            AclDecision::Admit
        );
        assert_eq!(
            evaluate_full(&policy, "10.1.2.3".parse().unwrap(), &store),
            AclDecision::Refuse
        );
    }

    #[test]
    fn tag_action_carries_tag_name() {
        let store = store_with_cidr("corp_nets", "10.0.0.0/8", "1");
        let cfg = AclsConfig {
            default_action: "allow".into(),
            rules: vec![AclRule {
                r#match: "corp_nets".into(),
                action: "tag".into(),
                tag: Some("internal".into()),
            }],
        };
        let policy = CompiledAclPolicy::compile(&cfg);
        assert_eq!(
            evaluate_full(&policy, "10.1.2.3".parse().unwrap(), &store),
            AclDecision::Tag("internal".into())
        );
    }

    #[test]
    fn check_client_acl_reports_all_listeners_and_filter() {
        let dir = std::env::temp_dir().join(format!(
            "conduit-core-acl-check-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nets.txt");
        std::fs::write(&path, "10.0.0.0/8 1\n").unwrap();
        let yaml = format!(
            r#"schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "0.0.0.0:53"
      protocol: udp
      name: public
      acls:
        default_action: deny
        rules:
          - match: corp_nets
            action: accept
    - address: "10.0.0.1:53"
      protocol: udp
      name: internal
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
data_sources:
  - name: corp_nets
    type: cidr
    path: "{}"
acls:
  default_action: allow
  rules:
    - match: corp_nets
      action: tag
      tag: corp
"#,
            path.display()
        );
        let cfg = conduit_config::load_yaml(&yaml).unwrap();
        let scripting = conduit_script::compile_from_config(&cfg, None).unwrap();
        let store = &*scripting.data_sources;

        let all = check_client_acl(&cfg, store, "10.1.2.3".parse().unwrap(), None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].listener, "public");
        assert_eq!(all[0].decision, "admit");
        assert_eq!(all[0].action, "accept");
        assert_eq!(all[0].matched, "corp_nets");
        assert_eq!(all[1].listener, "internal");
        assert_eq!(all[1].decision, "tag");
        assert_eq!(all[1].tag.as_deref(), Some("corp"));

        let filtered =
            check_client_acl(&cfg, store, "203.0.113.1".parse().unwrap(), Some("public")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].decision, "drop");
        assert_eq!(filtered[0].matched, "default");
        assert_eq!(filtered[0].action, "deny");

        let err = check_client_acl(&cfg, store, "10.1.2.3".parse().unwrap(), Some("missing"))
            .unwrap_err();
        assert!(matches!(err, AclCheckError::UnknownListener(_)));
    }
}
