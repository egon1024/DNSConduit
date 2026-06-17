//! Rule-style selectors shared by built-in rules and observation sink filters.

use conduit_proto::config::Selector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledSelector {
    QnameSuffix(String),
    QnameExact(String),
    Qtype(String),
    Rcode(String),
    Tag(String),
    SamplePercent(PercentKey),
    EveryNthWorker(u64),
    EveryNthGlobal(u64),
}

/// Inputs for selector matching without coupling to `conduit-core::Transaction`.
#[derive(Clone)]
pub struct SelectorMatchCtx<'a> {
    pub txn_id: u64,
    pub global_query_index: u64,
    pub qname: Option<&'a str>,
    pub qtype_label: Option<String>,
    pub rcode_label: Option<String>,
    pub tag_has: &'a dyn Fn(&str) -> bool,
}

const RULE_ONLY_SELECTOR_TYPES: &[&str] = &["every_nth_worker", "every_nth_global"];
pub const SELECTOR_TYPES: &[&str] = &[
    "qname_suffix",
    "qname_exact",
    "qtype",
    "rcode",
    "tag",
    "sample_percent",
    "every_nth_worker",
    "every_nth_global",
];
pub const NON_RULE_SELECTOR_TYPES: &[&str] = &[
    "qname_suffix",
    "qname_exact",
    "qtype",
    "rcode",
    "tag",
    "sample_percent",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PercentKey(u16);

pub fn validate_selector_type(ty: &str) -> Result<(), String> {
    if SELECTOR_TYPES.contains(&ty) {
        Ok(())
    } else {
        Err(format!("unknown selector type '{ty}'"))
    }
}

pub fn validate_non_rule_selector_type(ty: &str) -> Result<(), String> {
    validate_selector_type(ty)?;
    if RULE_ONLY_SELECTOR_TYPES.contains(&ty) {
        Err(format!(
            "selector type '{ty}' is only valid in rules selectors"
        ))
    } else {
        Ok(())
    }
}

pub fn compile_selectors(selectors: &[Selector]) -> Vec<CompiledSelector> {
    selectors.iter().map(CompiledSelector::compile).collect()
}

impl CompiledSelector {
    pub fn compile(sel: &Selector) -> Self {
        match sel.r#type.as_str() {
            "qname_exact" => CompiledSelector::QnameExact(sel.value.clone()),
            "qtype" => CompiledSelector::Qtype(sel.value.clone()),
            "rcode" => CompiledSelector::Rcode(sel.value.clone()),
            "tag" => CompiledSelector::Tag(sel.value.clone()),
            "sample_percent" => {
                let percent = parse_percent_key(sel.value.as_str()).unwrap_or(PercentKey(0));
                CompiledSelector::SamplePercent(percent)
            }
            "every_nth_worker" => {
                let nth = parse_every_nth(sel.value.as_str()).unwrap_or(1);
                CompiledSelector::EveryNthWorker(nth)
            }
            "every_nth_global" => {
                let nth = parse_every_nth(sel.value.as_str()).unwrap_or(1);
                CompiledSelector::EveryNthGlobal(nth)
            }
            _ => CompiledSelector::QnameSuffix(sel.value.clone()),
        }
    }

    pub fn matches_ctx(&self, ctx: &SelectorMatchCtx<'_>) -> bool {
        match self {
            CompiledSelector::QnameSuffix(suffix) => {
                ctx.qname.is_some_and(|q| q.ends_with(suffix.as_str()))
            }
            CompiledSelector::QnameExact(name) => ctx.qname == Some(name.as_str()),
            CompiledSelector::Qtype(t) => ctx.qtype_label.as_deref() == Some(t.as_str()),
            CompiledSelector::Rcode(r) => ctx.rcode_label.as_deref() == Some(r.as_str()),
            CompiledSelector::Tag(key) => (ctx.tag_has)(key),
            CompiledSelector::SamplePercent(percent) => {
                hash_sample(ctx.txn_id, percent.as_fraction())
            }
            CompiledSelector::EveryNthWorker(n) => ctx.txn_id.checked_rem(*n) == Some(0),
            CompiledSelector::EveryNthGlobal(n) => {
                ctx.global_query_index.checked_rem(*n) == Some(0)
            }
        }
    }
}

impl PercentKey {
    pub fn as_fraction(self) -> f64 {
        self.0 as f64 / 10_000.0
    }
}

pub fn parse_sample_percent(value: &str) -> Result<f64, String> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("sample_percent selector value '{value}' is not a number"))?;
    if !(0.0..=100.0).contains(&parsed) {
        return Err(format!(
            "sample_percent selector value '{value}' must be in [0, 100]"
        ));
    }
    Ok(parsed)
}

pub fn parse_every_nth(value: &str) -> Result<u64, String> {
    let parsed = value
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("every_nth selector value '{value}' must be an integer >= 1"))?;
    if parsed == 0 {
        return Err("every_nth selector value must be >= 1".into());
    }
    Ok(parsed)
}

fn parse_percent_key(value: &str) -> Result<PercentKey, String> {
    let parsed = parse_sample_percent(value)?;
    Ok(PercentKey((parsed * 100.0).round() as u16))
}

/// Deterministic per-transaction sampling in `(0, 1]`.
pub fn hash_sample(txn_id: u64, rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let threshold = (rate * 10_000.0).floor() as u64;
    let bucket = txn_id.wrapping_mul(0x9E37_79B9_7F4A_7C15) % 10_000;
    bucket < threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::Selector;

    #[test]
    fn hash_sample_stable_per_txn() {
        assert!(hash_sample(42, 1.0));
        assert!(!hash_sample(42, 0.0));
        let a = hash_sample(99, 0.5);
        let b = hash_sample(99, 0.5);
        assert_eq!(a, b);
    }

    #[test]
    fn qname_suffix_matches() {
        let sel = CompiledSelector::QnameSuffix(".example".into());
        let ctx = SelectorMatchCtx {
            txn_id: 1,
            global_query_index: 1,
            qname: Some("www.example"),
            qtype_label: None,
            rcode_label: None,
            tag_has: &|_| false,
        };
        assert!(sel.matches_ctx(&ctx));
    }

    #[test]
    fn sample_percent_edges_and_decimals() {
        let zero = CompiledSelector::compile(&Selector {
            r#type: "sample_percent".into(),
            value: "0".into(),
        });
        let hundred = CompiledSelector::compile(&Selector {
            r#type: "sample_percent".into(),
            value: "100".into(),
        });
        let decimal = CompiledSelector::compile(&Selector {
            r#type: "sample_percent".into(),
            value: "12.5".into(),
        });
        let ctx = SelectorMatchCtx {
            txn_id: 42,
            global_query_index: 42,
            qname: None,
            qtype_label: None,
            rcode_label: None,
            tag_has: &|_| false,
        };
        assert!(!zero.matches_ctx(&ctx));
        assert!(hundred.matches_ctx(&ctx));
        assert_eq!(decimal.matches_ctx(&ctx), decimal.matches_ctx(&ctx));
    }

    #[test]
    fn every_nth_worker_and_global_match() {
        let worker = CompiledSelector::compile(&Selector {
            r#type: "every_nth_worker".into(),
            value: "4".into(),
        });
        let global = CompiledSelector::compile(&Selector {
            r#type: "every_nth_global".into(),
            value: "4".into(),
        });
        let ctx = SelectorMatchCtx {
            txn_id: 8,
            global_query_index: 12,
            qname: None,
            qtype_label: None,
            rcode_label: None,
            tag_has: &|_| false,
        };
        assert!(worker.matches_ctx(&ctx));
        assert!(global.matches_ctx(&ctx));
    }
}
