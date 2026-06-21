//! Rule-style selectors shared by built-in rules and observation sink filters.

use conduit_dns_wire::parse_selector_wire_value;
use conduit_proto::config::Selector;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledSelector {
    QnameSuffix(String),
    QnameExact(String),
    Qtype(u16),
    Rcode(u16),
    Qclass(u16),
    Opcode(u8),
    EdnsOption(u16),
    Tag(String),
    SamplePercent { percent: PercentKey, key: SampleKey },
    EveryNthWorker(u64),
    EveryNthGlobal(u64),
}

/// Salt for deterministic `sample_percent` bucketing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SampleKey {
    #[default]
    Global,
    Literal(String),
    FromQname,
}

/// Context for resolving `key_from` when compiling selectors.
#[derive(Debug, Clone, Default)]
pub struct SelectorCompileCtx {
    pub rule_name: Option<String>,
    pub sink_name: Option<String>,
}

/// Inputs for selector matching without coupling to `conduit-core::Transaction`.
#[derive(Clone)]
pub struct SelectorMatchCtx<'a> {
    pub txn_id: u64,
    pub global_query_index: u64,
    pub qname: Option<&'a str>,
    pub qtype: Option<u16>,
    pub rcode: Option<u16>,
    pub qclass: Option<u16>,
    pub opcode: Option<u8>,
    pub edns_option_codes: &'a [u16],
    pub tag_has: &'a dyn Fn(&str) -> bool,
}

const RULE_ONLY_SELECTOR_TYPES: &[&str] = &["every_nth_worker", "every_nth_global"];
pub const SELECTOR_TYPES: &[&str] = &[
    "qname_suffix",
    "qname_exact",
    "qtype",
    "rcode",
    "qclass",
    "opcode",
    "edns_option",
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
    "qclass",
    "opcode",
    "edns_option",
    "tag",
    "sample_percent",
];

pub const SAMPLE_KEY_FROM_QNAME: &str = "qname";
pub const SAMPLE_KEY_FROM_RULE_NAME: &str = "rule_name";
pub const SAMPLE_KEY_FROM_SINK_NAME: &str = "sink_name";

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

pub fn validate_wire_selector_value(selector_type: &str, value: &str) -> Result<(), String> {
    match selector_type {
        "qtype" | "rcode" | "qclass" | "opcode" | "edns_option" => {
            parse_selector_wire_value(selector_type, value).map(|_| ())
        }
        _ => Ok(()),
    }
}

/// Compile selectors for observation sinks and tracing (no `rule_name` binding).
pub fn compile_selectors(selectors: &[Selector]) -> Result<Vec<CompiledSelector>, String> {
    compile_selectors_with_ctx(selectors, &SelectorCompileCtx::default())
}

/// Compile selectors for a built-in rule (`key_from: rule_name` resolves here).
pub fn compile_rule_selectors(
    rule_name: &str,
    selectors: &[Selector],
) -> Result<Vec<CompiledSelector>, String> {
    compile_selectors_with_ctx(
        selectors,
        &SelectorCompileCtx {
            rule_name: Some(rule_name.to_string()),
            sink_name: None,
        },
    )
}

/// Compile selectors for an event sink filter list (`key_from: sink_name` resolves here).
pub fn compile_sink_selectors(
    sink_name: &str,
    selectors: &[Selector],
) -> Result<Vec<CompiledSelector>, String> {
    compile_selectors_with_ctx(
        selectors,
        &SelectorCompileCtx {
            rule_name: None,
            sink_name: Some(sink_name.to_string()),
        },
    )
}

pub fn compile_selectors_with_ctx(
    selectors: &[Selector],
    ctx: &SelectorCompileCtx,
) -> Result<Vec<CompiledSelector>, String> {
    selectors
        .iter()
        .map(|s| CompiledSelector::compile(s, ctx))
        .collect()
}

pub fn compile_sample_key_fields(
    key: Option<&str>,
    key_from: Option<&str>,
    ctx: &SelectorCompileCtx,
    allow_rule_name: bool,
    allow_sink_name: bool,
) -> Result<SampleKey, String> {
    let key = key.filter(|k| !k.is_empty());
    let key_from = key_from.filter(|k| !k.is_empty());
    match (key, key_from) {
        (Some(_), Some(_)) => Err("sample key and key_from are mutually exclusive".into()),
        (Some(literal), None) => Ok(SampleKey::Literal(literal.to_string())),
        (None, Some(from)) => compile_sample_key_from(from, ctx, allow_rule_name, allow_sink_name),
        (None, None) => Ok(SampleKey::Global),
    }
}

fn compile_sample_key_from(
    key_from: &str,
    ctx: &SelectorCompileCtx,
    allow_rule_name: bool,
    allow_sink_name: bool,
) -> Result<SampleKey, String> {
    match key_from {
        SAMPLE_KEY_FROM_QNAME => Ok(SampleKey::FromQname),
        SAMPLE_KEY_FROM_RULE_NAME if allow_rule_name => {
            let name = ctx.rule_name.as_deref().ok_or_else(|| {
                "sample_percent key_from rule_name requires a rule compile context".to_string()
            })?;
            Ok(SampleKey::Literal(name.to_string()))
        }
        SAMPLE_KEY_FROM_SINK_NAME if allow_sink_name => {
            let name = ctx.sink_name.as_deref().ok_or_else(|| {
                "sample_percent key_from sink_name requires a sink compile context".to_string()
            })?;
            Ok(SampleKey::Literal(name.to_string()))
        }
        SAMPLE_KEY_FROM_RULE_NAME => Err(format!(
            "sample_percent key_from '{SAMPLE_KEY_FROM_RULE_NAME}' is only valid on rule selectors"
        )),
        SAMPLE_KEY_FROM_SINK_NAME => Err(format!(
            "sample_percent key_from '{SAMPLE_KEY_FROM_SINK_NAME}' is only valid on event sink filters"
        )),
        other => Err(format!(
            "sample_percent key_from '{other}' must be {SAMPLE_KEY_FROM_QNAME}, {SAMPLE_KEY_FROM_RULE_NAME}, or {SAMPLE_KEY_FROM_SINK_NAME}"
        )),
    }
}

pub fn validate_selector_sample_key_fields(
    sel: &Selector,
    allow_rule_name: bool,
    allow_sink_name: bool,
) -> Result<(), String> {
    if sel.r#type != "sample_percent" {
        if sel.key.as_ref().is_some_and(|k| !k.is_empty())
            || sel.key_from.as_ref().is_some_and(|k| !k.is_empty())
        {
            return Err(format!(
                "selector type '{}' does not support key or key_from",
                sel.r#type
            ));
        }
        return Ok(());
    }
    let key = sel.key.as_deref().filter(|k| !k.is_empty());
    let key_from = sel.key_from.as_deref().filter(|k| !k.is_empty());
    match (key, key_from) {
        (Some(_), Some(_)) => Err("sample key and key_from are mutually exclusive".into()),
        (Some(_), None) => Ok(()),
        (None, Some(from)) => validate_sample_key_from(from, allow_rule_name, allow_sink_name),
        (None, None) => Ok(()),
    }
}

pub fn validate_sample_key_from(
    key_from: &str,
    allow_rule_name: bool,
    allow_sink_name: bool,
) -> Result<(), String> {
    match key_from {
        SAMPLE_KEY_FROM_QNAME => Ok(()),
        SAMPLE_KEY_FROM_RULE_NAME if allow_rule_name => Ok(()),
        SAMPLE_KEY_FROM_SINK_NAME if allow_sink_name => Ok(()),
        SAMPLE_KEY_FROM_RULE_NAME => Err(format!(
            "sample_percent key_from '{SAMPLE_KEY_FROM_RULE_NAME}' is only valid on rule selectors"
        )),
        SAMPLE_KEY_FROM_SINK_NAME => Err(format!(
            "sample_percent key_from '{SAMPLE_KEY_FROM_SINK_NAME}' is only valid on event sink filters"
        )),
        other => Err(format!(
            "sample_percent key_from '{other}' must be {SAMPLE_KEY_FROM_QNAME}, {SAMPLE_KEY_FROM_RULE_NAME}, or {SAMPLE_KEY_FROM_SINK_NAME}"
        )),
    }
}

pub fn validate_top_level_sample_key_fields(
    key: Option<&str>,
    key_from: Option<&str>,
    allow_sink_name: bool,
) -> Result<(), String> {
    let key = key.filter(|k| !k.is_empty());
    let key_from = key_from.filter(|k| !k.is_empty());
    match (key, key_from) {
        (Some(_), Some(_)) => Err("sample_key and sample_key_from are mutually exclusive".into()),
        (Some(_), None) => Ok(()),
        (None, Some(from)) => validate_sample_key_from(from, false, allow_sink_name),
        (None, None) => Ok(()),
    }
}

impl CompiledSelector {
    pub fn compile(sel: &Selector, ctx: &SelectorCompileCtx) -> Result<Self, String> {
        match sel.r#type.as_str() {
            "qname_exact" => Ok(CompiledSelector::QnameExact(sel.value.clone())),
            "qtype" => parse_selector_wire_value("qtype", &sel.value).map(CompiledSelector::Qtype),
            "rcode" => parse_selector_wire_value("rcode", &sel.value).map(CompiledSelector::Rcode),
            "qclass" => {
                parse_selector_wire_value("qclass", &sel.value).map(CompiledSelector::Qclass)
            }
            "opcode" => parse_selector_wire_value("opcode", &sel.value)
                .map(|n| CompiledSelector::Opcode(n as u8)),
            "edns_option" => {
                parse_selector_wire_value("edns_option", &sel.value).map(CompiledSelector::EdnsOption)
            }
            "tag" => Ok(CompiledSelector::Tag(sel.value.clone())),
            "sample_percent" => {
                let percent = parse_percent_key(sel.value.as_str())?;
                let key = compile_sample_key_fields(
                    sel.key.as_deref(),
                    sel.key_from.as_deref(),
                    ctx,
                    ctx.rule_name.is_some(),
                    ctx.sink_name.is_some(),
                )?;
                Ok(CompiledSelector::SamplePercent { percent, key })
            }
            "every_nth_worker" => {
                let nth = parse_every_nth(sel.value.as_str())?;
                Ok(CompiledSelector::EveryNthWorker(nth))
            }
            "every_nth_global" => {
                let nth = parse_every_nth(sel.value.as_str())?;
                Ok(CompiledSelector::EveryNthGlobal(nth))
            }
            _ => Ok(CompiledSelector::QnameSuffix(sel.value.clone())),
        }
    }

    pub fn matches_ctx(&self, ctx: &SelectorMatchCtx<'_>) -> bool {
        match self {
            CompiledSelector::QnameSuffix(suffix) => {
                ctx.qname.is_some_and(|q| q.ends_with(suffix.as_str()))
            }
            CompiledSelector::QnameExact(name) => ctx.qname == Some(name.as_str()),
            CompiledSelector::Qtype(wire) => ctx.qtype == Some(*wire),
            CompiledSelector::Rcode(wire) => ctx.rcode == Some(*wire),
            CompiledSelector::Qclass(wire) => ctx.qclass == Some(*wire),
            CompiledSelector::Opcode(wire) => ctx.opcode == Some(*wire),
            CompiledSelector::EdnsOption(wire) => ctx
                .edns_option_codes
                .iter()
                .any(|code| *code == *wire),
            CompiledSelector::Tag(key) => (ctx.tag_has)(key),
            CompiledSelector::SamplePercent { percent, key } => {
                if matches!(key, SampleKey::FromQname) && ctx.qname.is_none() {
                    return false;
                }
                let salt = resolve_sample_key(key, ctx);
                hash_sample_keyed(ctx.txn_id, percent.as_fraction(), salt.as_deref())
            }
            CompiledSelector::EveryNthWorker(n) => matches_every_nth_worker(ctx.txn_id, *n),
            CompiledSelector::EveryNthGlobal(n) => {
                matches_every_nth_global(ctx.global_query_index, *n)
            }
        }
    }
}

pub fn resolve_sample_key(key: &SampleKey, ctx: &SelectorMatchCtx<'_>) -> Option<String> {
    match key {
        SampleKey::Global => None,
        SampleKey::Literal(s) => Some(s.clone()),
        SampleKey::FromQname => ctx.qname.map(str::to_string),
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

/// Whether `txn_id` matches YAML `every_nth_worker` (`txn_id % nth == 0`).
pub fn matches_every_nth_worker(txn_id: u64, nth: u64) -> bool {
    nth >= 1 && txn_id.checked_rem(nth) == Some(0)
}

/// Whether `global_query_index` matches YAML `every_nth_global` (`index % nth == 0`).
pub fn matches_every_nth_global(global_query_index: u64, nth: u64) -> bool {
    nth >= 1 && global_query_index.checked_rem(nth) == Some(0)
}

fn parse_percent_key(value: &str) -> Result<PercentKey, String> {
    let parsed = parse_sample_percent(value)?;
    Ok(PercentKey((parsed * 100.0).round() as u16))
}

/// Deterministic per-transaction sampling in `(0, 1]` using the global bucket (no salt).
pub fn hash_sample(txn_id: u64, rate: f64) -> bool {
    hash_sample_keyed(txn_id, rate, None)
}

/// Deterministic sampling: optional `key` salt selects an independent bucket namespace.
pub fn hash_sample_keyed(txn_id: u64, rate: f64, key: Option<&str>) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let threshold = (rate * 10_000.0).floor() as u64;
    let bucket = sample_bucket(txn_id, key);
    bucket < threshold
}

fn sample_bucket(txn_id: u64, key: Option<&str>) -> u64 {
    match key.filter(|k| !k.is_empty()) {
        None => txn_id.wrapping_mul(0x9E37_79B9_7F4A_7C15) % 10_000,
        Some(salt) => {
            let mut hasher = DefaultHasher::new();
            txn_id.hash(&mut hasher);
            salt.hash(&mut hasher);
            hasher.finish() % 10_000
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::Selector;

    fn test_ctx<'a>(
        txn_id: u64,
        global_query_index: u64,
        qname: Option<&'a str>,
        qtype: Option<u16>,
        rcode: Option<u16>,
        tag_has: &'a dyn Fn(&str) -> bool,
    ) -> SelectorMatchCtx<'a> {
        SelectorMatchCtx {
            txn_id,
            global_query_index,
            qname,
            qtype,
            rcode,
            qclass: None,
            opcode: None,
            edns_option_codes: &[],
            tag_has,
        }
    }

    #[test]
    fn hash_sample_stable_per_txn() {
        assert!(hash_sample(42, 1.0));
        assert!(!hash_sample(42, 0.0));
        let a = hash_sample(99, 0.5);
        let b = hash_sample(99, 0.5);
        assert_eq!(a, b);
    }

    #[test]
    fn keyed_sample_differs_by_salt_and_is_stable() {
        let rate = 0.5;
        let keyed_a = hash_sample_keyed(4242, rate, Some("zone-a"));
        assert_eq!(keyed_a, hash_sample_keyed(4242, rate, Some("zone-a")));
        let differs = (1..10_000u64).any(|txn_id| {
            hash_sample_keyed(txn_id, rate, None) != hash_sample_keyed(txn_id, rate, Some("salt"))
        });
        assert!(
            differs,
            "keyed and global buckets should diverge for some txn ids"
        );
        let salt_differs = (1..10_000u64).any(|txn_id| {
            hash_sample_keyed(txn_id, rate, Some("zone-a"))
                != hash_sample_keyed(txn_id, rate, Some("zone-b"))
        });
        assert!(
            salt_differs,
            "different salts should diverge for some txn ids"
        );
    }

    #[test]
    fn qname_suffix_matches() {
        let sel = CompiledSelector::QnameSuffix(".example".into());
        let ctx = test_ctx(1, 1, Some("www.example"), None, None, &|_| false);
        assert!(sel.matches_ctx(&ctx));
    }

    #[test]
    fn qtype_matches_wire_number_and_type_alias() {
        let from_name = CompiledSelector::compile(
            &Selector {
                r#type: "qtype".into(),
                value: "A".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let from_alias = CompiledSelector::compile(
            &Selector {
                r#type: "qtype".into(),
                value: "TYPE1".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        assert_eq!(from_name, from_alias);
        let ctx = test_ctx(1, 1, None, Some(1), None, &|_| false);
        assert!(from_name.matches_ctx(&ctx));
        assert!(!from_name.matches_ctx(&test_ctx(1, 1, None, Some(28), None, &|_| false)));
    }

    #[test]
    fn sample_percent_edges_and_decimals() {
        let zero = CompiledSelector::compile(
            &Selector {
                r#type: "sample_percent".into(),
                value: "0".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let hundred = CompiledSelector::compile(
            &Selector {
                r#type: "sample_percent".into(),
                value: "100".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let decimal = CompiledSelector::compile(
            &Selector {
                r#type: "sample_percent".into(),
                value: "12.5".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let ctx = test_ctx(42, 42, None, None, None, &|_| false);
        assert!(!zero.matches_ctx(&ctx));
        assert!(hundred.matches_ctx(&ctx));
        assert_eq!(decimal.matches_ctx(&ctx), decimal.matches_ctx(&ctx));
    }

    #[test]
    fn sample_percent_static_key_compiles() {
        let sel = CompiledSelector::compile(
            &Selector {
                r#type: "sample_percent".into(),
                value: "25".into(),
                key: Some("internal.example".into()),
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        assert!(matches!(
            sel,
            CompiledSelector::SamplePercent {
                key: SampleKey::Literal(ref k),
                ..
            } if k == "internal.example"
        ));
    }

    #[test]
    fn sample_percent_key_from_rule_name_bakes_at_compile() {
        let sel = CompiledSelector::compile(
            &Selector {
                r#type: "sample_percent".into(),
                value: "10".into(),
                key: None,
                key_from: Some("rule_name".into()),
            },
            &SelectorCompileCtx {
                rule_name: Some("my-rule".into()),
                sink_name: None,
            },
        )
        .unwrap();
        assert!(matches!(
            sel,
            CompiledSelector::SamplePercent {
                key: SampleKey::Literal(ref k),
                ..
            } if k == "my-rule"
        ));
    }

    #[test]
    fn sample_percent_key_from_qname_uses_query_name() {
        let sel = CompiledSelector::compile(
            &Selector {
                r#type: "sample_percent".into(),
                value: "100".into(),
                key: None,
                key_from: Some("qname".into()),
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let ctx = test_ctx(7, 7, Some("foo.example."), None, None, &|_| false);
        assert!(sel.matches_ctx(&ctx));
        let no_qname = test_ctx(7, 7, None, None, None, &|_| false);
        assert!(!sel.matches_ctx(&no_qname));
    }

    #[test]
    fn every_nth_worker_and_global_match() {
        let worker = CompiledSelector::compile(
            &Selector {
                r#type: "every_nth_worker".into(),
                value: "4".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let global = CompiledSelector::compile(
            &Selector {
                r#type: "every_nth_global".into(),
                value: "4".into(),
                key: None,
                key_from: None,
            },
            &SelectorCompileCtx::default(),
        )
        .unwrap();
        let ctx = test_ctx(8, 12, None, None, None, &|_| false);
        assert!(worker.matches_ctx(&ctx));
        assert!(global.matches_ctx(&ctx));
    }

    #[test]
    fn reject_key_on_non_sample_selector() {
        let err = validate_selector_sample_key_fields(
            &Selector {
                r#type: "qname_suffix".into(),
                value: ".example.".into(),
                key: Some("x".into()),
                key_from: None,
            },
            true,
            false,
        )
        .unwrap_err();
        assert!(err.contains("does not support key"));
    }
}
