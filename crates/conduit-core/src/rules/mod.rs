//! Built-in selector/action rules compiled at snapshot build (spec §6).

use crate::selector_ctx::selector_match_ctx;
use crate::transaction::Transaction;
use conduit_events::{compile_rule_selectors, CompiledSelector};
use conduit_metrics::{BuiltinProfile, BuiltinRegistry, MetricsHub, UserRegistry};
use conduit_proto::config::{Action, Rule, RulesConfig};
use conduit_script::{
    run_scripts, CompiledScripting, RoutingRuntimeSnapshot, ScriptPhase, ScriptRunOutcome,
};
use std::sync::Arc;

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
    SetTag {
        key: String,
        value: String,
    },
    /// Pool for retry Route if retry occurs; first Route ignores (both hooks).
    SetRetryPool(String),
    /// Soft retry — remaining actions on this rule still run; resolved at end of rule.
    Retry,
    /// Hard retry — stop further actions on this rule (soft drop still wins).
    RetryNow,
    /// Soft drop — remaining actions on this rule still run; resolved at end of rule.
    Drop,
    /// Hard drop — stop further actions on this rule.
    DropNow,
    /// Clear soft-drop intent set earlier on this rule.
    ClearDrop,
    /// Clear soft-retry intent set earlier on this rule (response hook only).
    ClearRetry,
    /// Clear `retry_pool` on the transaction.
    ClearRetryPool,
    /// Clear `selected_pool` so [Route] uses the configured default pool.
    ClearPool,
    /// Remove a tag key from the transaction (bool and string).
    ClearTag(String),
    SetRcode(u16),
    SetSourceV4(std::net::Ipv4Addr),
    SetSourceV6(std::net::Ipv6Addr),
    /// One-shot IPv4 egress for next retry forward if retry occurs; first forward ignores.
    SetRetrySourceV4(std::net::Ipv4Addr),
    /// One-shot IPv6 egress for next retry forward if retry occurs; first forward ignores.
    SetRetrySourceV6(std::net::Ipv6Addr),
    /// Clear `retry_source_override_v4` on the transaction.
    ClearRetrySourceV4,
    /// Clear `retry_source_override_v6` on the transaction.
    ClearRetrySourceV6,
    Rhai {
        script_id: usize,
    },
}

impl CompiledRules {
    pub fn compile(cfg: Option<&RulesConfig>, scripting: &CompiledScripting) -> Self {
        let Some(cfg) = cfg else {
            return Self {
                match_mode: MatchMode::FirstMatch,
                rules: Vec::new(),
            };
        };
        Self {
            match_mode: MatchMode::FirstMatch,
            rules: cfg
                .rules
                .iter()
                .map(|rule| CompiledRule::compile(rule, scripting))
                .collect(),
        }
    }

    pub fn eval(
        &self,
        hook: RuleHook,
        txn: &mut Transaction,
        scripting: &CompiledScripting,
        metrics: Option<&MetricsHub>,
        routing_runtime: Option<Arc<RoutingRuntimeSnapshot>>,
    ) -> RuleEvalResult {
        let user_export = metrics.map(|m| m.user.as_ref());
        let builtin_profile = metrics.map(|m| m.compiled.profile);
        let builtin = metrics.map(|m| Arc::clone(&m.builtin));
        for rule in self.rules.iter().filter(|r| r.hook == hook) {
            if rule.matches(txn) {
                let outcome = rule.execute_ordered(
                    txn,
                    scripting,
                    user_export,
                    builtin_profile,
                    builtin.clone(),
                    routing_runtime.clone(),
                );
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
    fn compile(rule: &Rule, scripting: &CompiledScripting) -> Self {
        let hook = if rule.hook == "response" {
            RuleHook::Response
        } else {
            RuleHook::Request
        };
        let script_phase = match hook {
            RuleHook::Request => ScriptPhase::Request,
            RuleHook::Response => ScriptPhase::Response,
        };
        let script_ids = scripting.script_ids_for_rule(&rule.name, script_phase);
        let mut rhai_idx = 0usize;
        let actions = rule
            .actions
            .iter()
            .map(|act| {
                if act.r#type == "rhai" {
                    let script_id = script_ids.get(rhai_idx).copied().unwrap_or_else(|| {
                        panic!(
                            "missing compiled rhai script for rule '{}' action {}",
                            rule.name, rhai_idx
                        )
                    });
                    rhai_idx += 1;
                    CompiledAction::Rhai { script_id }
                } else {
                    CompiledAction::compile(act)
                }
            })
            .collect();
        Self {
            name: rule.name.clone(),
            hook,
            selectors: compile_rule_selectors(&rule.name, &rule.selectors)
                .unwrap_or_else(|e| panic!("rule '{}': {e}", rule.name)),
            actions,
        }
    }

    fn matches(&self, txn: &Transaction) -> bool {
        if self.selectors.is_empty() {
            return true;
        }
        let tag_has = |k: &str| txn.tags.has(k);
        let ctx = selector_match_ctx(txn, &tag_has);
        self.selectors.iter().all(|s| s.matches_ctx(&ctx))
    }

    fn execute_ordered(
        &self,
        txn: &mut Transaction,
        scripting: &CompiledScripting,
        user_export: Option<&UserRegistry>,
        builtin_profile: Option<BuiltinProfile>,
        builtin: Option<Arc<BuiltinRegistry>>,
        routing_runtime: Option<Arc<RoutingRuntimeSnapshot>>,
    ) -> RuleOutcome {
        let script_phase = match self.hook {
            RuleHook::Request => ScriptPhase::Request,
            RuleHook::Response => ScriptPhase::Response,
        };
        let mut retry = false;

        for action in &self.actions {
            match action {
                CompiledAction::Rhai { script_id } => {
                    let (script_outcome, stats) = run_scripts(
                        scripting,
                        &[*script_id],
                        txn,
                        script_phase,
                        user_export,
                        builtin_profile,
                        builtin.clone(),
                        routing_runtime.clone(),
                    );
                    match script_outcome {
                        ScriptRunOutcome::DropNow => return RuleOutcome::Drop,
                        ScriptRunOutcome::Drop => {}
                        ScriptRunOutcome::Retry => {
                            if self.hook == RuleHook::Response {
                                retry = true;
                            }
                        }
                        ScriptRunOutcome::RetryNow => {
                            if self.hook == RuleHook::Response {
                                return self.retry_outcome(txn);
                            }
                        }
                        ScriptRunOutcome::Error => return self.final_outcome(txn, retry),
                        ScriptRunOutcome::Ok => {
                            if stats.clear_soft_retry {
                                retry = false;
                            }
                        }
                    }
                }
                _ => {
                    if let Some(stop) = self.apply_builtin(action, txn, &mut retry) {
                        return stop;
                    }
                }
            }
        }

        self.final_outcome(txn, retry)
    }

    fn apply_builtin(
        &self,
        action: &CompiledAction,
        txn: &mut Transaction,
        retry: &mut bool,
    ) -> Option<RuleOutcome> {
        match action {
            CompiledAction::SetPool(p) => txn.selected_pool = Some(p.clone()),
            CompiledAction::SetTag { key, value } => {
                txn.tags.set_string(key, value);
            }
            CompiledAction::SetRetryPool(p) => {
                txn.retry_pool = Some(p.clone());
            }
            CompiledAction::Retry => *retry = true,
            CompiledAction::RetryNow => return Some(self.retry_outcome(txn)),
            CompiledAction::Drop => txn.set_soft_drop(),
            CompiledAction::DropNow => return Some(RuleOutcome::Drop),
            CompiledAction::ClearDrop => txn.clear_soft_drop(),
            CompiledAction::ClearRetry => *retry = false,
            CompiledAction::ClearRetryPool => txn.clear_retry_pool(),
            CompiledAction::ClearPool => txn.clear_pool(),
            CompiledAction::ClearTag(key) => txn.tags.clear(key),
            CompiledAction::SetRcode(rc) => txn.set_rcode(*rc),
            CompiledAction::SetSourceV4(addr) => txn.set_source_override_v4(*addr),
            CompiledAction::SetSourceV6(addr) => txn.set_source_override_v6(*addr),
            CompiledAction::SetRetrySourceV4(addr) => txn.set_retry_source_override_v4(*addr),
            CompiledAction::SetRetrySourceV6(addr) => txn.set_retry_source_override_v6(*addr),
            CompiledAction::ClearRetrySourceV4 => txn.clear_retry_source_override_v4(),
            CompiledAction::ClearRetrySourceV6 => txn.clear_retry_source_override_v6(),
            CompiledAction::Rhai { .. } => unreachable!(),
        }
        None
    }

    fn final_outcome(&self, txn: &Transaction, retry: bool) -> RuleOutcome {
        if txn.soft_drop {
            RuleOutcome::Drop
        } else if retry {
            RuleOutcome::Retry
        } else {
            RuleOutcome::Continue
        }
    }

    fn retry_outcome(&self, txn: &Transaction) -> RuleOutcome {
        if txn.soft_drop {
            RuleOutcome::Drop
        } else {
            RuleOutcome::Retry
        }
    }
}

impl CompiledAction {
    fn compile(act: &Action) -> Self {
        match act.r#type.as_str() {
            "set_pool" => CompiledAction::SetPool(act.value.clone()),
            "set_retry_pool" => CompiledAction::SetRetryPool(act.value.clone()),
            "retry" => CompiledAction::Retry,
            "retry_now" => CompiledAction::RetryNow,
            "drop" => CompiledAction::Drop,
            "drop_now" => CompiledAction::DropNow,
            "clear_drop" => CompiledAction::ClearDrop,
            "clear_retry" => CompiledAction::ClearRetry,
            "clear_retry_pool" => CompiledAction::ClearRetryPool,
            "clear_pool" => CompiledAction::ClearPool,
            "clear_tag" => CompiledAction::ClearTag(act.value.clone()),
            "set_rcode" => CompiledAction::SetRcode(
                conduit_dns_wire::Rcode::parse_name_or_err(&act.value)
                    .map(conduit_dns_wire::Rcode::number)
                    .expect("set_rcode must be validated before compile"),
            ),
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
            "set_retry_source_v4" => CompiledAction::SetRetrySourceV4(
                act.value
                    .parse()
                    .expect("set_retry_source_v4 must be validated before compile"),
            ),
            "set_retry_source_v6" => CompiledAction::SetRetrySourceV6(
                act.value
                    .parse()
                    .expect("set_retry_source_v6 must be validated before compile"),
            ),
            "clear_retry_source_v4" => CompiledAction::ClearRetrySourceV4,
            "clear_retry_source_v6" => CompiledAction::ClearRetrySourceV6,
            "rhai" => panic!("rhai actions are compiled via CompiledRule::compile"),
            other => panic!("unknown action type '{other}' must be rejected at validate"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{ClientProtocol, Transaction};
    use conduit_proto::config::{Action, Rule, RulesConfig, Selector};
    use std::net::SocketAddr;

    fn empty_scripting() -> CompiledScripting {
        conduit_script::compile_from_config(
            &conduit_config::load_yaml(
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
        weight: 100
"#,
            )
            .unwrap(),
            None,
        )
        .unwrap()
    }

    fn compile_request_rule(actions: Vec<Action>) -> CompiledRules {
        compile_hook_rule("request", actions)
    }

    fn compile_response_rule(actions: Vec<Action>) -> CompiledRules {
        compile_hook_rule("response", actions)
    }

    fn compile_hook_rule(hook: &str, actions: Vec<Action>) -> CompiledRules {
        let scripting = empty_scripting();
        CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![Rule {
                    name: "test".into(),
                    hook: hook.into(),
                    selectors: vec![],
                    actions,
                }],
            }),
            &scripting,
        )
    }

    fn eval_request(rules: &CompiledRules, txn: &mut Transaction) -> RuleEvalResult {
        let scripting = empty_scripting();
        rules.eval(RuleHook::Request, txn, &scripting, None, None)
    }

    fn eval_response(rules: &CompiledRules, txn: &mut Transaction) -> RuleEvalResult {
        let scripting = empty_scripting();
        rules.eval(RuleHook::Response, txn, &scripting, None, None)
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
        eval_request(&rules, &mut txn);
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
        let result = eval_response(&rules, &mut txn);
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
        eval_request(&rules, &mut txn);
        assert_eq!(txn.selected_pool.as_deref(), Some("internal"));
        assert_eq!(txn.source_override_v4, Some("192.0.2.1".parse().unwrap()));
    }

    #[test]
    fn set_retry_pool_stashes_without_retry() {
        for hook in ["request", "response"] {
            let rules = compile_hook_rule(
                hook,
                vec![Action {
                    r#type: "set_retry_pool".into(),
                    value: "secondary".into(),
                }],
            );
            let mut txn = Transaction::new(
                1,
                "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
                ClientProtocol::Udp,
            );
            let result = if hook == "request" {
                eval_request(&rules, &mut txn)
            } else {
                eval_response(&rules, &mut txn)
            };
            assert_eq!(result.outcome, RuleOutcome::Continue, "hook={hook}");
            assert_eq!(txn.retry_pool.as_deref(), Some("secondary"));
        }
    }

    #[test]
    fn set_retry_pool_then_retry_on_response() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "set_retry_pool".into(),
                value: "secondary".into(),
            },
            Action {
                r#type: "retry".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Retry);
        assert_eq!(txn.retry_pool.as_deref(), Some("secondary"));
    }

    #[test]
    fn retry_now_short_circuits_remaining_actions() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "retry_now".into(),
                value: "".into(),
            },
            Action {
                r#type: "set_tag".into(),
                value: "skipped=true".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Retry);
        assert!(!txn.tags.has("skipped"));
    }

    #[test]
    fn soft_drop_wins_over_retry_now() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "drop".into(),
                value: "".into(),
            },
            Action {
                r#type: "retry_now".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Drop);
    }

    #[test]
    fn soft_drop_continues_then_resolves_at_end() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "drop".into(),
                value: "".into(),
            },
            Action {
                r#type: "set_tag".into(),
                value: "audited=true".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_request(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Drop);
        assert!(txn.tags.has("audited"));
    }

    #[test]
    fn clear_drop_cancels_soft_drop() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "drop".into(),
                value: "".into(),
            },
            Action {
                r#type: "clear_drop".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_request(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Continue);
        assert!(!txn.soft_drop);
    }

    #[test]
    fn clear_retry_cancels_soft_retry() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "retry".into(),
                value: "".into(),
            },
            Action {
                r#type: "clear_retry".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Continue);
    }

    #[test]
    fn clear_retry_pool_clears_stash_not_retry_intent() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "set_retry_pool".into(),
                value: "secondary".into(),
            },
            Action {
                r#type: "retry".into(),
                value: "".into(),
            },
            Action {
                r#type: "clear_retry_pool".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Retry);
        assert!(txn.retry_pool.is_none());
    }

    #[test]
    fn clear_retry_pool_on_request_clears_stash() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "set_retry_pool".into(),
                value: "secondary".into(),
            },
            Action {
                r#type: "clear_retry_pool".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_request(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Continue);
        assert!(txn.retry_pool.is_none());
    }

    #[test]
    fn clear_pool_clears_selected_pool() {
        for hook in ["request", "response"] {
            let rules = compile_hook_rule(
                hook,
                vec![
                    Action {
                        r#type: "set_pool".into(),
                        value: "vip".into(),
                    },
                    Action {
                        r#type: "clear_pool".into(),
                        value: "".into(),
                    },
                ],
            );
            let mut txn = Transaction::new(
                1,
                "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
                ClientProtocol::Udp,
            );
            txn.selected_pool = Some("primary".into());
            let result = if hook == "request" {
                eval_request(&rules, &mut txn)
            } else {
                eval_response(&rules, &mut txn)
            };
            assert_eq!(result.outcome, RuleOutcome::Continue, "hook={hook}");
            assert!(txn.selected_pool.is_none(), "hook={hook}");
        }
    }

    #[test]
    fn clear_pool_then_set_pool_wins() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "clear_pool".into(),
                value: "".into(),
            },
            Action {
                r#type: "set_pool".into(),
                value: "vip".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        eval_request(&rules, &mut txn);
        assert_eq!(txn.selected_pool.as_deref(), Some("vip"));
    }

    #[test]
    fn set_retry_source_v4_stashes_without_consuming() {
        for hook in ["request", "response"] {
            let rules = compile_hook_rule(
                hook,
                vec![Action {
                    r#type: "set_retry_source_v4".into(),
                    value: "10.0.0.5".into(),
                }],
            );
            let mut txn = Transaction::new(
                1,
                "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
                ClientProtocol::Udp,
            );
            txn.source_override_v4 = Some("127.0.0.1".parse().unwrap());
            let result = if hook == "request" {
                eval_request(&rules, &mut txn)
            } else {
                eval_response(&rules, &mut txn)
            };
            assert_eq!(result.outcome, RuleOutcome::Continue, "hook={hook}");
            assert_eq!(
                txn.retry_source_override_v4,
                Some("10.0.0.5".parse().unwrap())
            );
            assert_eq!(
                txn.take_effective_source_override_v4(),
                Some("127.0.0.1".parse().unwrap())
            );
            assert_eq!(
                txn.retry_source_override_v4,
                Some("10.0.0.5".parse().unwrap())
            );
        }
    }

    #[test]
    fn retry_source_consumed_on_retry_forward() {
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.source_override_v4 = Some("127.0.0.1".parse().unwrap());
        txn.set_retry_source_override_v4("10.0.0.5".parse().unwrap());
        txn.attempt_count = 2;
        assert_eq!(
            txn.take_effective_source_override_v4(),
            Some("10.0.0.5".parse().unwrap())
        );
        assert!(txn.retry_source_override_v4.is_none());
        assert_eq!(
            txn.take_effective_source_override_v4(),
            Some("127.0.0.1".parse().unwrap())
        );
    }

    #[test]
    fn clear_retry_source_v4_clears_stash() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "set_retry_source_v4".into(),
                value: "10.0.0.5".into(),
            },
            Action {
                r#type: "clear_retry_source_v4".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Continue);
        assert!(txn.retry_source_override_v4.is_none());
    }

    #[test]
    fn drop_now_short_circuits_remaining_actions() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "drop_now".into(),
                value: "".into(),
            },
            Action {
                r#type: "set_tag".into(),
                value: "skipped=true".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_request(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Drop);
        assert!(!txn.tags.has("skipped"));
    }

    #[test]
    fn drop_wins_over_retry_at_end() {
        let rules = compile_response_rule(vec![
            Action {
                r#type: "retry".into(),
                value: "".into(),
            },
            Action {
                r#type: "drop".into(),
                value: "".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_response(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Drop);
    }

    #[test]
    fn every_nth_worker_and_qtype_use_and_semantics() {
        let scripting = empty_scripting();
        let rules = CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![Rule {
                    name: "sampled-a".into(),
                    hook: "request".into(),
                    selectors: vec![
                        Selector {
                            r#type: "every_nth_worker".into(),
                            value: "4".into(),
                            key: None,
                            key_from: None,
                        },
                        Selector {
                            r#type: "qtype".into(),
                            value: "A".into(),
                            key: None,
                            key_from: None,
                        },
                    ],
                    actions: vec![Action {
                        r#type: "set_tag".into(),
                        value: "hit=true".into(),
                    }],
                }],
            }),
            &scripting,
        );

        let mut hit = Transaction::new(
            8,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        hit.qtype = Some(1);
        rules.eval(RuleHook::Request, &mut hit, &scripting, None, None);
        assert!(hit.tags.has("hit"));

        let mut miss = Transaction::new(
            8,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        miss.qtype = Some(28);
        rules.eval(RuleHook::Request, &mut miss, &scripting, None, None);
        assert!(!miss.tags.has("hit"));
    }

    #[test]
    fn every_nth_global_uses_stored_index() {
        let scripting = empty_scripting();
        let rules = CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![Rule {
                    name: "global-4th".into(),
                    hook: "request".into(),
                    selectors: vec![Selector {
                        r#type: "every_nth_global".into(),
                        value: "4".into(),
                        key: None,
                        key_from: None,
                    }],
                    actions: vec![Action {
                        r#type: "set_tag".into(),
                        value: "global=true".into(),
                    }],
                }],
            }),
            &scripting,
        );

        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        )
        .with_global_query_index(8);
        rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(txn.tags.has("global"));

        rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(txn.tags.has("global"));
        assert_eq!(txn.global_query_index, 8);
    }

    #[test]
    fn clear_tag_after_set_tag_removes_tag() {
        let rules = compile_request_rule(vec![
            Action {
                r#type: "set_tag".into(),
                value: "audited=true".into(),
            },
            Action {
                r#type: "clear_tag".into(),
                value: "audited".into(),
            },
        ]);
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let result = eval_request(&rules, &mut txn);
        assert_eq!(result.outcome, RuleOutcome::Continue);
        assert!(!txn.tags.has("audited"));
    }

    #[test]
    fn clear_tag_prevents_tag_selector_match() {
        let scripting = empty_scripting();
        let set_rules = CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![Rule {
                    name: "set-vip".into(),
                    hook: "request".into(),
                    selectors: vec![],
                    actions: vec![Action {
                        r#type: "set_tag".into(),
                        value: "vip=true".into(),
                    }],
                }],
            }),
            &scripting,
        );
        let clear_rules = CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![Rule {
                    name: "clear-vip".into(),
                    hook: "request".into(),
                    selectors: vec![],
                    actions: vec![Action {
                        r#type: "clear_tag".into(),
                        value: "vip".into(),
                    }],
                }],
            }),
            &scripting,
        );
        let match_rules = CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![Rule {
                    name: "vip-only".into(),
                    hook: "request".into(),
                    selectors: vec![Selector {
                        r#type: "tag".into(),
                        value: "vip".into(),
                        key: None,
                        key_from: None,
                    }],
                    actions: vec![Action {
                        r#type: "set_tag".into(),
                        value: "matched=true".into(),
                    }],
                }],
            }),
            &scripting,
        );

        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        set_rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(txn.tags.has("vip"));

        match_rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(txn.tags.has("matched"));

        clear_rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(!txn.tags.has("vip"));

        txn.tags.clear("matched");
        match_rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(!txn.tags.has("matched"));
    }

    #[test]
    fn keyed_sample_percent_rules_use_independent_buckets() {
        use conduit_events::hash_sample_keyed;

        let scripting = empty_scripting();
        let rules = CompiledRules::compile(
            Some(&RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![
                    Rule {
                        name: "zone-a".into(),
                        hook: "request".into(),
                        selectors: vec![Selector {
                            r#type: "sample_percent".into(),
                            value: "50".into(),
                            key: Some("zone-a".into()),
                            key_from: None,
                        }],
                        actions: vec![Action {
                            r#type: "set_tag".into(),
                            value: "a=true".into(),
                        }],
                    },
                    Rule {
                        name: "zone-b".into(),
                        hook: "request".into(),
                        selectors: vec![Selector {
                            r#type: "sample_percent".into(),
                            value: "50".into(),
                            key: Some("zone-b".into()),
                            key_from: None,
                        }],
                        actions: vec![Action {
                            r#type: "set_tag".into(),
                            value: "b=true".into(),
                        }],
                    },
                ],
            }),
            &scripting,
        );

        let txn_id = (1..10_000u64)
            .find(|id| {
                !hash_sample_keyed(*id, 0.5, Some("zone-a"))
                    && hash_sample_keyed(*id, 0.5, Some("zone-b"))
            })
            .expect("some txn should pass zone-b but not zone-a");

        let mut txn = Transaction::new(
            txn_id,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        rules.eval(RuleHook::Request, &mut txn, &scripting, None, None);
        assert!(txn.tags.has("b"));
        assert!(!txn.tags.has("a"));
    }
}
