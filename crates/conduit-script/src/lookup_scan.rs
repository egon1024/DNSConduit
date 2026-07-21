//! Static analysis of `lookup("literal", …)` and `lookup_ip("literal", …)` in Rhai sources.

use crate::data_sources::DataSourceStore;
use crate::error::ScriptError;
use std::collections::BTreeSet;

/// Extract distinct string-literal table names from `lookup("name", …)` calls.
pub fn scan_lookup_literals(source: &str) -> Vec<String> {
    scan_call_literals(source, "lookup")
}

/// Extract distinct string-literal view names from `lookup_ip("name", …)` calls.
pub fn scan_lookup_ip_literals(source: &str) -> Vec<String> {
    scan_call_literals(source, "lookup_ip")
}

pub fn validate_lookup_literals(
    source: &str,
    path: &str,
    store: &DataSourceStore,
) -> Result<(), ScriptError> {
    for name in scan_lookup_literals(source) {
        if !store.has_table(&name) {
            return Err(ScriptError::Rule {
                rule_name: path.into(),
                message: format!("unknown data source '{name}' in lookup"),
            });
        }
    }
    for name in scan_lookup_ip_literals(source) {
        if !store.has_cidr_table(&name) {
            return Err(ScriptError::Rule {
                rule_name: path.into(),
                message: format!("unknown data source '{name}' in lookup_ip"),
            });
        }
    }
    Ok(())
}

fn scan_call_literals(source: &str, fn_name: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for line in source.lines() {
        for rest in find_calls(line, fn_name) {
            if let Some(name) = extract_call_arg(rest) {
                names.insert(name);
            }
        }
    }
    names.into_iter().collect()
}

/// Finds occurrences of `fn_name` in `line` at identifier word boundaries
/// (so `lookup` does not match inside `lookup_ip`), returning the tail of
/// the line right after each match.
fn find_calls<'a>(line: &'a str, fn_name: &str) -> Vec<&'a str> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(rel) = line[start..].find(fn_name) {
        let idx = start + rel;
        let after = idx + fn_name.len();
        let before_ok = idx == 0 || !is_ident_byte(bytes[idx - 1]);
        let after_ok = bytes.get(after).map(|b| !is_ident_byte(*b)).unwrap_or(true);
        if before_ok && after_ok {
            out.push(&line[after..]);
        }
        start = after;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn extract_call_arg(rest: &str) -> Option<String> {
    let after_open = rest.trim_start().strip_prefix('(')?;
    let after_paren = after_open.trim_start();
    let quote = after_paren.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let content = after_paren[1..].split(quote).next()?;
    Some(content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_sources::DataSourceStore;
    use std::collections::HashMap;

    #[test]
    fn scan_finds_literal_table_names() {
        let src = r#"
if lookup("blocklist", txn.question().qname) == "block" { }
let x = lookup("geo", "key");
"#;
        let mut names = scan_lookup_literals(src);
        names.sort();
        assert_eq!(names, vec!["blocklist".to_string(), "geo".to_string()]);
    }

    #[test]
    fn scan_ignores_non_literal_first_arg() {
        let src = r#"lookup(t, "key");"#;
        assert!(scan_lookup_literals(src).is_empty());
    }

    #[test]
    fn scan_lookup_does_not_match_lookup_ip() {
        let src = r#"lookup_ip("corp_nets", txn.client_ip());"#;
        assert!(scan_lookup_literals(src).is_empty());
        assert_eq!(scan_lookup_ip_literals(src), vec!["corp_nets".to_string()]);
    }

    #[test]
    fn validate_rejects_unknown_table() {
        let store = DataSourceStore::default();
        let src = r#"lookup("typo", txn.question().qname);"#;
        let err = validate_lookup_literals(src, "test.rhai", &store).unwrap_err();
        assert!(err.to_string().contains("typo"));
    }

    #[test]
    fn validate_accepts_known_table() {
        let mut store = DataSourceStore::default();
        store.insert_table("blocklist", HashMap::new());
        let src = r#"lookup("blocklist", txn.question().qname);"#;
        validate_lookup_literals(src, "test.rhai", &store).unwrap();
    }

    #[test]
    fn validate_rejects_unknown_cidr_view() {
        let store = DataSourceStore::default();
        let src = r#"lookup_ip("corp_nets", txn.client_ip());"#;
        let err = validate_lookup_literals(src, "test.rhai", &store).unwrap_err();
        assert!(err.to_string().contains("corp_nets"));
    }

    #[test]
    fn validate_accepts_known_cidr_view() {
        let mut store = DataSourceStore::default();
        store.insert_cidr_table("corp_nets");
        let src = r#"lookup_ip("corp_nets", txn.client_ip());"#;
        validate_lookup_literals(src, "test.rhai", &store).unwrap();
    }
}
