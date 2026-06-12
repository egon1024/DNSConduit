//! Built-in selector/action rules compiled at snapshot build (spec §6).

use crate::transaction::Transaction;
use conduit_events::{compile_selectors, CompiledSelector, SelectorMatchCtx};
use conduit_proto::config::{Action, Rule, RulesConfig};

#[derive(Debug, Clone)]
pub struct CompiledRules {
    pub match_mode: MatchMode,
    pub rules: Vec<CompiledRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    FirstMatch,
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub name: String,
    pub hook: RuleHook,
    pub selectors: Vec<CompiledSelector>,
    pub actions: Vec<CompiledAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleHook {
    Request,
    Response,
}

#[derive(Debug, Clone)]
pub enum CompiledAction {
    SetPool(String),
    SetTag { key: String, value: String },
    RetryPool(String),
    Retry,
    Drop,
    SetRcode(String),
    SetSourceV4(std::net::Ipv4Addr),
    SetSourceV6(std::net::Ipv6Addr),
    Rhai,
}

impl CompiledRules {
    pub fn compile(cfg: Option<&RulesConfig>) -> Self {
        let Some(cfg) = cfg else {
            return Self {
                match_mode: MatchMode::FirstMatch,
                rules: Vec::new(),
            };
        };
        let match_mode = MatchMode::FirstMatch;
        Self {
            match_mode,
            rules: cfg.rules.iter().map(CompiledRule::compile).collect(),
        }
    }

    pub fn eval(&self, hook: RuleHook, txn: &mut Transaction) -> RuleEvalResult {
        for rule in self.rules.iter().filter(|r| r.hook == hook) {
            if rule.matches(txn) {
                let outcome = rule.apply(txn);
                if self.match_mode == MatchMode::FirstMatch {
                    return RuleEvalResult {
                        outcome,
                        matched_rule_name: Some(rule.name.clone()),
                    };
                }
            }
        }
        RuleEvalResult {
            outcome: RuleOutcome::Continue,
            matched_rule_name: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuleEvalResult {
    pub outcome: RuleOutcome,
    pub matched_rule_name: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuleOutcome {
    Continue,
    Drop,
    Retry,
}

impl CompiledRule {
    fn compile(rule: &Rule) -> Self {
        let hook = if rule.hook == "response" {
            RuleHook::Response
        } else {
            RuleHook::Request
        };
        Self {
            name: rule.name.clone(),
            hook,
            selectors: compile_selectors(&rule.selectors),
            actions: rule.actions.iter().map(CompiledAction::compile).collect(),
        }
    }

    fn matches(&self, txn: &Transaction) -> bool {
        if self.selectors.is_empty() {
            return true;
        }
        let ctx = SelectorMatchCtx {
            qname: txn.qname.as_deref(),
            qtype_label: txn.qtype_label(),
            rcode_label: txn.rcode_label(),
            tag_has: &|k| txn.tags.has(k),
        };
        self.selectors.iter().all(|s| s.matches_ctx(&ctx))
    }

    fn apply(&self, txn: &mut Transaction) -> RuleOutcome {
        let mut retry = false;
        let mut drop = false;
        for action in &self.actions {
            match action {
                CompiledAction::Rhai => {}
                CompiledAction::SetPool(p) => txn.selected_pool = Some(p.clone()),
                CompiledAction::SetTag { key, value } => {
                    txn.tags.set_string(key, value);
                }
                CompiledAction::RetryPool(p) => {
                    txn.retry_pool = Some(p.clone());
                    retry = true;
                }
                CompiledAction::Retry => retry = true,
                CompiledAction::Drop => drop = true,
                CompiledAction::SetRcode(rc) => txn.set_rcode_name(rc),
                CompiledAction::SetSourceV4(addr) => txn.set_source_override_v4(*addr),
                CompiledAction::SetSourceV6(addr) => txn.set_source_override_v6(*addr),
            }
        }
        if drop {
            RuleOutcome::Drop
        } else if retry {
            RuleOutcome::Retry
        } else {
            RuleOutcome::Continue
        }
    }
}

impl CompiledAction {
    fn compile(act: &Action) -> Self {
        match act.r#type.as_str() {
            "set_pool" => CompiledAction::SetPool(act.value.clone()),
            "retry_pool" => CompiledAction::RetryPool(act.value.clone()),
            "retry" => CompiledAction::Retry,
            "drop" => CompiledAction::Drop,
            "set_rcode" => CompiledAction::SetRcode(act.value.clone()),
            "set_tag" => {
                let (key, value) = act
                    .value
                    .split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .unwrap_or((act.value.clone(), "true".into()));
                CompiledAction::SetTag { key, value }
            }
            "set_source_v4" => CompiledAction::SetSourceV4(
                act.value
                    .parse()
                    .expect("set_source_v4 must be validated before compile"),
            ),
            "set_source_v6" => CompiledAction::SetSourceV6(
                act.value
                    .parse()
                    .expect("set_source_v6 must be validated before compile"),
            ),
            "rhai" => CompiledAction::Rhai,
            _ => CompiledAction::SetPool(act.value.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{ClientProtocol, Transaction};
    use conduit_proto::config::{Action, Rule, RulesConfig};
    use std::net::SocketAddr;

    fn compile_request_rule(actions: Vec<Action>) -> CompiledRules {
        compile_hook_rule("request", actions)
    }

    fn compile_response_rule(actions: Vec<Action>) -> CompiledRules {
        compile_hook_rule("response", actions)
    }

    fn compile_hook_rule(hook: &str, actions: Vec<Action>) -> CompiledRules {
        CompiledRules::compile(Some(&RulesConfig {
            match_mode: "first_match".into(),
            rules: vec![Rule {
                name: "test".into(),
                hook: hook.into(),
                selectors: vec![],
                actions,
            }],
        }))
    }

    #[test]
    fn set_source_v4_action_sets_override() {
        let rules = compile_request_rule(vec![Action {
            r#type: "set_source_v4".into(),
            value: "10.0.0.5".into(),
        }]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        rules.eval(RuleHook::Request, &mut txn);
        assert_eq!(txn.source_override_v4, Some("10.0.0.5".parse().unwrap()));
    }

    #[test]
    fn retry_action_requests_route_without_pool() {
        let rules = compile_response_rule(vec![Action {
            r#type: "retry".into(),
            value: "".into(),
        }]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = Some("primary".into());
        txn.set_rcode_name("SERVFAIL");
        let result = rules.eval(RuleHook::Response, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Retry);
        assert!(txn.retry_pool.is_none());
    }

    #[test]
    fn set_pool_before_set_source_applies_in_order() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "set_pool".into(),
                value: "internal".into(),
            },
            Action {
                r#type: "set_source_v4".into(),
                value: "192.0.2.1".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        rules.eval(RuleHook::Request, &mut txn);
        assert_eq!(txn.selected_pool.as_deref(), Some("internal"));
        assert_eq!(txn.source_override_v4, Some("192.0.2.1".parse().unwrap()));
    }
}
