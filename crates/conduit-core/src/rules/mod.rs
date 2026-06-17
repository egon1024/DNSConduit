//! Built-in selector/action rules compiled at snapshot build (spec §6).

use crate::transaction::Transaction;
use conduit_events::{compile_selectors, CompiledSelector, SelectorMatchCtx};
use conduit_metrics::UserRegistry;
use conduit_proto::config::{Action, Rule, RulesConfig};
use conduit_script::{run_scripts, CompiledScripting, ScriptPhase, ScriptRunOutcome};

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
    SetRcode(String),
    SetSourceV4(std::net::Ipv4Addr),
    SetSourceV6(std::net::Ipv6Addr),
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
        user_export: Option<&UserRegistry>,
    ) -> RuleEvalResult {
        for rule in self.rules.iter().filter(|r| r.hook == hook) {
            if rule.matches(txn) {
                let outcome = rule.execute_ordered(txn, scripting, user_export);
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
            selectors: compile_selectors(&rule.selectors),
            actions,
        }
    }

    fn matches(&self, txn: &Transaction) -> bool {
        if self.selectors.is_empty() {
            return true;
        }
        let ctx = SelectorMatchCtx {
            txn_id: txn.id,
            global_query_index: txn.global_query_index,
            qname: txn.qname.as_deref(),
            qtype_label: txn.qtype_label(),
            rcode_label: txn.rcode_label(),
            tag_has: &|k| txn.tags.has(k),
        };
        self.selectors.iter().all(|s| s.matches_ctx(&ctx))
    }

    fn execute_ordered(
        &self,
        txn: &mut Transaction,
        scripting: &CompiledScripting,
        user_export: Option<&UserRegistry>,
    ) -> RuleOutcome {
        let script_phase = match self.hook {
            RuleHook::Request => ScriptPhase::Request,
            RuleHook::Response => ScriptPhase::Response,
        };
        let mut retry = false;

        for action in &self.actions {
            match action {
                CompiledAction::Rhai { script_id } => {
                    let (script_outcome, stats) =
                        run_scripts(scripting, &[*script_id], txn, script_phase, user_export);
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
            CompiledAction::SetRcode(rc) => txn.set_rcode_name(rc),
            CompiledAction::SetSourceV4(addr) => txn.set_source_override_v4(*addr),
            CompiledAction::SetSourceV6(addr) => txn.set_source_override_v6(*addr),
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
        rules.eval(RuleHook::Request, txn, &scripting, None)
    }

    fn eval_response(rules: &CompiledRules, txn: &mut Transaction) -> RuleEvalResult {
        let scripting = empty_scripting();
        rules.eval(RuleHook::Response, txn, &scripting, None)
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
                        },
                        Selector {
                            r#type: "qtype".into(),
                            value: "A".into(),
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
        rules.eval(RuleHook::Request, &mut hit, &scripting, None);
        assert!(hit.tags.has("hit"));

        let mut miss = Transaction::new(
            8,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        miss.qtype = Some(28);
        rules.eval(RuleHook::Request, &mut miss, &scripting, None);
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
        rules.eval(RuleHook::Request, &mut txn, &scripting, None);
        assert!(txn.tags.has("global"));

        rules.eval(RuleHook::Request, &mut txn, &scripting, None);
        assert!(txn.tags.has("global"));
        assert_eq!(txn.global_query_index, 8);
    }
}
