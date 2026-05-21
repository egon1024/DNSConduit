//! Built-in selector/action rules compiled at snapshot build (spec §6).

use crate::transaction::Transaction;
use conduit_proto::config::{Action, Rule, RulesConfig, Selector};

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
    pub id: String,
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
pub enum CompiledSelector {
    QnameSuffix(String),
    QnameExact(String),
    Qtype(String),
    Rcode(String),
    Tag(String),
}

#[derive(Debug, Clone)]
pub enum CompiledAction {
    SetPool(String),
    SetTag { key: String, value: String },
    RetryPool(String),
    Drop,
    SetRcode(String),
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

    pub fn eval(&self, hook: RuleHook, txn: &mut Transaction) -> RuleOutcome {
        for rule in self.rules.iter().filter(|r| r.hook == hook) {
            if rule.matches(txn) {
                let outcome = rule.apply(txn);
                if self.match_mode == MatchMode::FirstMatch {
                    return outcome;
                }
            }
        }
        RuleOutcome::Continue
    }
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
            id: rule.id.clone(),
            hook,
            selectors: rule
                .selectors
                .iter()
                .map(CompiledSelector::compile)
                .collect(),
            actions: rule.actions.iter().map(CompiledAction::compile).collect(),
        }
    }

    fn matches(&self, txn: &Transaction) -> bool {
        if self.selectors.is_empty() {
            return true;
        }
        self.selectors.iter().all(|s| s.matches(txn))
    }

    fn apply(&self, txn: &mut Transaction) -> RuleOutcome {
        let mut retry = false;
        let mut drop = false;
        for action in &self.actions {
            match action {
                CompiledAction::SetPool(p) => txn.selected_pool = Some(p.clone()),
                CompiledAction::SetTag { key, value } => {
                    txn.tags.set_string(key, value);
                }
                CompiledAction::RetryPool(p) => {
                    txn.retry_pool = Some(p.clone());
                    retry = true;
                }
                CompiledAction::Drop => drop = true,
                CompiledAction::SetRcode(rc) => txn.set_rcode_name(rc),
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

impl CompiledSelector {
    fn compile(sel: &Selector) -> Self {
        match sel.r#type.as_str() {
            "qname_exact" => CompiledSelector::QnameExact(sel.value.clone()),
            "qtype" => CompiledSelector::Qtype(sel.value.clone()),
            "rcode" => CompiledSelector::Rcode(sel.value.clone()),
            "tag" => CompiledSelector::Tag(sel.value.clone()),
            _ => CompiledSelector::QnameSuffix(sel.value.clone()),
        }
    }

    fn matches(&self, txn: &Transaction) -> bool {
        match self {
            CompiledSelector::QnameSuffix(suffix) => {
                txn.qname.as_deref().is_some_and(|q| q.ends_with(suffix))
            }
            CompiledSelector::QnameExact(name) => txn.qname.as_deref() == Some(name.as_str()),
            CompiledSelector::Qtype(t) => txn.qtype_label().as_deref() == Some(t.as_str()),
            CompiledSelector::Rcode(r) => txn.rcode_label().as_deref() == Some(r.as_str()),
            CompiledSelector::Tag(key) => txn.tags.has(key),
        }
    }
}

impl CompiledAction {
    fn compile(act: &Action) -> Self {
        match act.r#type.as_str() {
            "set_pool" => CompiledAction::SetPool(act.value.clone()),
            "retry_pool" => CompiledAction::RetryPool(act.value.clone()),
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
            _ => CompiledAction::SetPool(act.value.clone()),
        }
    }
}
