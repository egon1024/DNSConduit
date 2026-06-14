use crate::error::ScriptError;
use conduit_proto::config::DataSource;
use conduit_proto::paths::resolve_config_path;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct DataSourceStore {
    tables: HashMap<String, HashMap<String, String>>,
}

impl DataSourceStore {
    pub fn lookup(&self, table: &str, key: &str) -> String {
        self.tables
            .get(table)
            .and_then(|t| t.get(key))
            .cloned()
            .unwrap_or_default()
    }

    pub fn has_table(&self, table: &str) -> bool {
        self.tables.contains_key(table)
    }

    pub fn table_names(&self) -> impl Iterator<Item = &String> {
        self.tables.keys()
    }
}

pub fn load_data_sources(
    sources: &[DataSource],
    base_dir: Option<&Path>,
) -> Result<DataSourceStore, ScriptError> {
    let mut store = DataSourceStore::default();
    for ds in sources {
        if ds.r#type != "csv" {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: format!("unsupported type '{}', only csv is supported", ds.r#type),
            });
        }
        if ds.name.is_empty() {
            return Err(ScriptError::DataSource {
                name: String::new(),
                message: "name must not be empty".into(),
            });
        }
        if ds.path.is_empty() {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: "path must not be empty".into(),
            });
        }
        let path = resolve_config_path(base_dir, &ds.path);
        let table = load_csv(&path, ds)?;
        store.tables.insert(ds.name.clone(), table);
    }
    Ok(store)
}

fn load_csv(path: &Path, ds: &DataSource) -> Result<HashMap<String, String>, ScriptError> {
    let content = std::fs::read_to_string(path).map_err(|e| ScriptError::DataSource {
        name: ds.name.clone(),
        message: format!("failed to read {}: {e}", path.display()),
    })?;
    let key_col = column_spec(&ds.key_column, 0);
    let value_col = column_spec(&ds.value_column, 1);

    let mut table = HashMap::new();
    let mut header: Option<Vec<String>> = None;
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = parse_csv_line(line);
        if cols.is_empty() {
            continue;
        }
        if header.is_none() && looks_like_header(&cols, &key_col, &value_col) {
            header = Some(cols.iter().map(|c| c.to_string()).collect());
            continue;
        }
        let (key_idx, val_idx) = if let Some(ref hdr) = header {
            (
                hdr.iter().position(|h| h == &key_col).unwrap_or(0),
                hdr.iter().position(|h| h == &value_col).unwrap_or(1),
            )
        } else {
            (0, 1)
        };
        if key_idx >= cols.len() || val_idx >= cols.len() {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: format!("line {}: not enough columns", line_no + 1),
            });
        }
        table.insert(cols[key_idx].to_string(), cols[val_idx].to_string());
    }
    Ok(table)
}

fn column_spec(name: &str, default_index: usize) -> String {
    if name.is_empty() {
        match default_index {
            0 => "key".into(),
            1 => "value".into(),
            n => n.to_string(),
        }
    } else {
        name.to_string()
    }
}

fn looks_like_header(cols: &[&str], key_col: &str, value_col: &str) -> bool {
    cols.iter().any(|c| *c == key_col || *c == value_col)
        || cols
            .first()
            .is_some_and(|k| k.eq_ignore_ascii_case("qname") || k.eq_ignore_ascii_case("key"))
}

fn parse_csv_line(line: &str) -> Vec<&str> {
    line.split(',').map(|s| s.trim()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/data")
            .join(name)
    }

    #[test]
    fn load_blocklist_csv_relative_to_base_dir() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/data");
        let ds = DataSource {
            name: "blocklist".into(),
            r#type: "csv".into(),
            path: "blocklist.csv".into(),
            key_column: "qname".into(),
            value_column: "action".into(),
        };
        let store = load_data_sources(&[ds], Some(base.as_path())).unwrap();
        assert_eq!(store.lookup("blocklist", "bad.example."), "block");
    }

    #[test]
    fn load_blocklist_csv() {
        let ds = DataSource {
            name: "blocklist".into(),
            r#type: "csv".into(),
            path: fixture_path("blocklist.csv").display().to_string(),
            key_column: "qname".into(),
            value_column: "action".into(),
        };
        let table = load_csv(Path::new(&ds.path), &ds).unwrap();
        assert_eq!(table.get("bad.example.").map(String::as_str), Some("block"));
        assert_eq!(
            table.get("good.example.").map(String::as_str),
            Some("allow")
        );
    }

    #[test]
    fn lookup_miss_returns_empty() {
        let mut store = DataSourceStore::default();
        store
            .tables
            .insert("t".into(), HashMap::from([("k".into(), "v".into())]));
        assert_eq!(store.lookup("t", "missing"), "");
        assert_eq!(store.lookup("unknown", "k"), "");
    }
}
