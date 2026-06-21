//! Static analysis of Rhai scripts for optional runtime features (response wire parsing).

/// Markers that indicate a response-hook script reads upstream wire-derived metadata.
///
/// Conservative: any match enables wire parsing for the whole snapshot (all queries).
const RESPONSE_WIRE_META_MARKERS: &[&str] = &[
    "answer_count",
    "authority_count",
    "additional_count",
    "truncated",
    "authoritative",
    "response_has_answer",
    "response_truncated",
    "response_answer_count",
    "response_authority_count",
    "response_additional_count",
    "response_authoritative",
];

/// Returns true when `source` appears to use upstream response wire metadata.
pub fn script_needs_response_wire_meta(source: &str) -> bool {
    RESPONSE_WIRE_META_MARKERS
        .iter()
        .any(|marker| source.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_truncated_access() {
        let src = r#"
let r = txn.response()?;
if r.truncated { txn.request_retry(); }
"#;
        assert!(script_needs_response_wire_meta(src));
    }

    #[test]
    fn ignores_rcode_only_script() {
        let src = r#"
if txn.response_rcode() == Rcode::SERVFAIL { txn.request_retry(); }
"#;
        assert!(!script_needs_response_wire_meta(src));
    }

    #[test]
    fn detects_answer_count_method() {
        let src = r#"if txn.response_answer_count() == 0 { txn.request_retry(); }"#;
        assert!(script_needs_response_wire_meta(src));
    }
}
