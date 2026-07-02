//! Static analysis of `lookup("literal", …)` in Rhai sources.

use crate::data_sources::DataSourceStore;
use crate::error::ScriptError;
use std::collections::BTreeSet;

/// Extract distinct string-literal table names from `lookup("name", …)` calls.
pub fn scan_lookup_literals(source: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for line in source.lines() {
        if let Some(name) = extract_lookup_literal(line) {
            names.insert(name);
        }
    }
    names.into_iter().collect()
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
    Ok(())
}

fn extract_lookup_literal(line: &str) -> Option<String> {
    let idx = line.find("lookup")?;
    let rest = &line[idx + "lookup".len()..];
    let open = rest.find('(')? + 1;
    let after_paren = rest[open..].trim_start();
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
}
