//! Rule-style selectors shared by built-in rules and observation sink filters.

use conduit_proto::config::Selector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledSelector {
    QnameSuffix(String),
    QnameExact(String),
    Qtype(String),
    Rcode(String),
    Tag(String),
}

/// Inputs for selector matching without coupling to `conduit-core::Transaction`.
#[derive(Clone)]
pub struct SelectorMatchCtx<'a> {
    pub qname: Option<&'a str>,
    pub qtype_label: Option<String>,
    pub rcode_label: Option<String>,
    pub tag_has: &'a dyn Fn(&str) -> bool,
}

pub const SELECTOR_TYPES: &[&str] = &["qname_suffix", "qname_exact", "qtype", "rcode", "tag"];

pub fn validate_selector_type(ty: &str) -> Result<(), String> {
    if SELECTOR_TYPES.contains(&ty) {
        Ok(())
    } else {
        Err(format!("unknown selector type '{ty}'"))
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
        }
    }
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
            qname: Some("www.example"),
            qtype_label: None,
            rcode_label: None,
            tag_has: &|_| false,
        };
        assert!(sel.matches_ctx(&ctx));
    }
}
