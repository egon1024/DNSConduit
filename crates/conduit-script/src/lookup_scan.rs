//! Static analysis of `table_lookup("literal", …)` in Rhai sources.

use crate::data_sources::DataSourceStore;
use crate::error::ScriptError;
use std::collections::HashSet;

/// Extract distinct string-literal table names from `table_lookup("name", …)` calls.
pub fn scan_table_lookup_literals(source: &str) -> Vec<String> {
    let mut found = HashSet::new();
    for line in source.lines() {
        if let Some(name) = extract_table_lookup_literal(line) {
            found.insert(name);
        }
    }
    found.into_iter().collect()
}

pub fn validate_table_lookup_literals(
    source: &str,
    path: &str,
    store: &DataSourceStore,
) -> Result<(), ScriptError> {
    for name in scan_table_lookup_literals(source) {
        if !store.has_table(&name) {
            return Err(ScriptError::Script {
                path: path.to_string(),
                message: format!("unknown data source '{name}' in table_lookup"),
            });
        }
    }
    Ok(())
}

fn extract_table_lookup_literal(line: &str) -> Option<String> {
    let idx = line.find("table_lookup")?;
    let rest = &line[idx..];
    let open = rest.find('(')?;
    let after = rest[open + 1..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let after_quote = &after[1..];
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_sources::DataSourceStore;
    use std::collections::HashMap;

    #[test]
    fn scan_finds_literal_table_names() {
        let src = r#"
if table_lookup("blocklist", question_qname(txn)) == "block" { }
let x = table_lookup("geo", "key");
"#;
        let mut names = scan_table_lookup_literals(src);
        names.sort();
        assert_eq!(names, vec!["blocklist".to_string(), "geo".to_string()]);
    }

    #[test]
    fn scan_ignores_dynamic_table_argument() {
        let src = r#"table_lookup(t, "key");"#;
        assert!(scan_table_lookup_literals(src).is_empty());
    }

    #[test]
    fn validate_rejects_unknown_literal() {
        let mut store = DataSourceStore::default();
        store.insert_table("blocklist", HashMap::from([("k".into(), "v".into())]));
        let src = r#"table_lookup("typo", question_qname(txn));"#;
        let err = validate_table_lookup_literals(src, "test.rhai", &store).unwrap_err();
        assert!(err.to_string().contains("unknown data source 'typo'"));
    }

    #[test]
    fn validate_accepts_configured_literal() {
        let mut store = DataSourceStore::default();
        store.insert_table("blocklist", HashMap::from([("k".into(), "v".into())]));
        let src = r#"table_lookup("blocklist", question_qname(txn));"#;
        validate_table_lookup_literals(src, "test.rhai", &store).unwrap();
    }
}
