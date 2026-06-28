use crate::error::ScriptError;
use conduit_proto::config::{DataSource, DataSourceLimits as ProtoDataSourceLimits};
use conduit_proto::paths::resolve_config_path;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Built-in load-safety defaults (generous; tunable via `data_source_limits:`).
/// Bound snapshot-compile memory so an oversized table file cannot OOM the
/// process during validate/reload/apply.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB per table file
pub const DEFAULT_MAX_ENTRIES: u64 = 1_000_000; // key->value pairs per table
pub const DEFAULT_MAX_KEY_BYTES: u32 = 1024; // 1 KiB per key
pub const DEFAULT_MAX_VALUE_BYTES: u32 = 4096; // 4 KiB per value
pub const DEFAULT_MAX_TABLES: u32 = 64; // entries in data_sources:
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB aggregate

/// Resolved, type-agnostic load-safety limits for `data_sources`.
///
/// Framed on the table/key->value abstraction (not on any file format) so the
/// same limits apply to future source types. Resolved from the global
/// `data_source_limits:` block via [`DataSourceLimits::from_config`]; per-entry
/// overrides are applied per table by [`DataSourceLimits::effective_for`].
#[derive(Debug, Clone, Copy)]
pub struct DataSourceLimits {
    pub max_file_bytes: u64,
    pub max_entries: u64,
    pub max_key_bytes: u32,
    pub max_value_bytes: u32,
    pub max_tables: u32,
    pub max_total_bytes: u64,
}

impl Default for DataSourceLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_key_bytes: DEFAULT_MAX_KEY_BYTES,
            max_value_bytes: DEFAULT_MAX_VALUE_BYTES,
            max_tables: DEFAULT_MAX_TABLES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
        }
    }
}

impl DataSourceLimits {
    /// Resolve the global block, treating `0` as "use the built-in default"
    /// (matching the `RhaiConfig`/`ScriptLimits` convention).
    pub fn from_config(cfg: Option<&ProtoDataSourceLimits>) -> Self {
        let d = Self::default();
        let Some(c) = cfg else {
            return d;
        };
        Self {
            max_file_bytes: nz_u64(c.max_file_bytes, d.max_file_bytes),
            max_entries: nz_u64(c.max_entries, d.max_entries),
            max_key_bytes: nz_u32(c.max_key_bytes, d.max_key_bytes),
            max_value_bytes: nz_u32(c.max_value_bytes, d.max_value_bytes),
            max_tables: nz_u32(c.max_tables, d.max_tables),
            max_total_bytes: nz_u64(c.max_total_bytes, d.max_total_bytes),
        }
    }

    /// Per-table effective limits: a per-entry override wins when set to a
    /// non-zero value, otherwise the resolved global value applies.
    fn effective_for(&self, ds: &DataSource) -> EffectiveTableLimits {
        EffectiveTableLimits {
            max_file_bytes: ds
                .max_file_bytes
                .filter(|v| *v != 0)
                .unwrap_or(self.max_file_bytes),
            max_entries: ds
                .max_entries
                .filter(|v| *v != 0)
                .unwrap_or(self.max_entries),
            max_key_bytes: ds
                .max_key_bytes
                .filter(|v| *v != 0)
                .unwrap_or(self.max_key_bytes),
            max_value_bytes: ds
                .max_value_bytes
                .filter(|v| *v != 0)
                .unwrap_or(self.max_value_bytes),
        }
    }
}

/// Per-table limits after applying per-entry overrides over the global block.
#[derive(Debug, Clone, Copy)]
struct EffectiveTableLimits {
    max_file_bytes: u64,
    max_entries: u64,
    max_key_bytes: u32,
    max_value_bytes: u32,
}

#[inline]
fn nz_u64(v: u64, default: u64) -> u64 {
    if v == 0 {
        default
    } else {
        v
    }
}

#[inline]
fn nz_u32(v: u32, default: u32) -> u32 {
    if v == 0 {
        default
    } else {
        v
    }
}

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

    #[cfg(test)]
    pub fn insert_table(&mut self, name: impl Into<String>, entries: HashMap<String, String>) {
        self.tables.insert(name.into(), entries);
    }
}

pub fn load_data_sources(
    sources: &[DataSource],
    base_dir: Option<&Path>,
    limits: &DataSourceLimits,
) -> Result<DataSourceStore, ScriptError> {
    if sources.len() as u64 > limits.max_tables as u64 {
        return Err(ScriptError::DataSource {
            name: String::new(),
            message: format!(
                "too many data_sources entries: {} exceeds max_tables {}",
                sources.len(),
                limits.max_tables
            ),
        });
    }
    let mut store = DataSourceStore::default();
    let mut total_bytes: u64 = 0;
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
        let effective = limits.effective_for(ds);
        let (table, bytes_read) = load_csv(&path, ds, &effective)?;
        total_bytes = total_bytes.saturating_add(bytes_read);
        if total_bytes > limits.max_total_bytes {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: format!(
                    "data_sources aggregate size {total_bytes} bytes exceeds max_total_bytes {}",
                    limits.max_total_bytes
                ),
            });
        }
        store.tables.insert(ds.name.clone(), table);
    }
    Ok(store)
}

/// Parse one CSV table, enforcing per-table load-safety limits. Returns the
/// table and the number of bytes read (for aggregate accounting).
fn load_csv(
    path: &Path,
    ds: &DataSource,
    limits: &EffectiveTableLimits,
) -> Result<(HashMap<String, String>, u64), ScriptError> {
    let read_err = |e: std::io::Error| ScriptError::DataSource {
        name: ds.name.clone(),
        message: format!("failed to read {}: {e}", path.display()),
    };
    let file = std::fs::File::open(path).map_err(read_err)?;
    // Read at most max_file_bytes + 1 so an oversized file is detected without
    // an unbounded read (and robust to growth between stat and read).
    let read_cap = limits.max_file_bytes.saturating_add(1);
    let mut buf = Vec::new();
    file.take(read_cap)
        .read_to_end(&mut buf)
        .map_err(read_err)?;
    if buf.len() as u64 > limits.max_file_bytes {
        return Err(ScriptError::DataSource {
            name: ds.name.clone(),
            message: format!(
                "file {} exceeds max_file_bytes {}",
                path.display(),
                limits.max_file_bytes
            ),
        });
    }
    let bytes_read = buf.len() as u64;
    let content = String::from_utf8(buf).map_err(|e| ScriptError::DataSource {
        name: ds.name.clone(),
        message: format!("failed to read {}: invalid UTF-8: {e}", path.display()),
    })?;
    let key_col = column_spec(&ds.key_column, 0);
    let value_col = column_spec(&ds.value_column, 1);

    let mut table = HashMap::new();
    let mut header: Option<Vec<String>> = None;
    let mut entry_count: u64 = 0;
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
        let key = cols[key_idx];
        let value = cols[val_idx];
        if key.len() as u64 > limits.max_key_bytes as u64 {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: format!(
                    "line {}: key length {} exceeds max_key_bytes {}",
                    line_no + 1,
                    key.len(),
                    limits.max_key_bytes
                ),
            });
        }
        if value.len() as u64 > limits.max_value_bytes as u64 {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: format!(
                    "line {}: value length {} exceeds max_value_bytes {}",
                    line_no + 1,
                    value.len(),
                    limits.max_value_bytes
                ),
            });
        }
        entry_count += 1;
        if entry_count > limits.max_entries {
            return Err(ScriptError::DataSource {
                name: ds.name.clone(),
                message: format!("exceeds max_entries {}", limits.max_entries),
            });
        }
        table.insert(key.to_string(), value.to_string());
    }
    Ok((table, bytes_read))
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
    use std::io::Write;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/data")
            .join(name)
    }

    fn csv_source(name: &str, path: String) -> DataSource {
        DataSource {
            name: name.into(),
            r#type: "csv".into(),
            path,
            key_column: "key".into(),
            value_column: "value".into(),
            max_file_bytes: None,
            max_entries: None,
            max_key_bytes: None,
            max_value_bytes: None,
        }
    }

    /// Write `content` to a unique temp file and return its path. The file is
    /// left for the OS temp reaper; tests do not depend on cleanup.
    fn temp_csv(tag: &str, content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "conduit-ds-{tag}-{}-{:?}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
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
            max_file_bytes: None,
            max_entries: None,
            max_key_bytes: None,
            max_value_bytes: None,
        };
        let store =
            load_data_sources(&[ds], Some(base.as_path()), &DataSourceLimits::default()).unwrap();
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
            max_file_bytes: None,
            max_entries: None,
            max_key_bytes: None,
            max_value_bytes: None,
        };
        let eff = DataSourceLimits::default().effective_for(&ds);
        let (table, _bytes) = load_csv(Path::new(&ds.path), &ds, &eff).unwrap();
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

    #[test]
    fn from_config_treats_zero_as_default() {
        let proto = ProtoDataSourceLimits {
            max_file_bytes: 0,
            max_entries: 5,
            max_key_bytes: 0,
            max_value_bytes: 0,
            max_tables: 0,
            max_total_bytes: 0,
        };
        let limits = DataSourceLimits::from_config(Some(&proto));
        assert_eq!(limits.max_file_bytes, DEFAULT_MAX_FILE_BYTES);
        assert_eq!(limits.max_entries, 5);
        assert_eq!(limits.max_tables, DEFAULT_MAX_TABLES);
    }

    #[test]
    fn oversize_file_rejected() {
        let big = "k,v\n".repeat(100);
        let path = temp_csv("oversize", &big);
        let ds = csv_source("t", path.display().to_string());
        let limits = DataSourceLimits {
            max_file_bytes: 8,
            ..DataSourceLimits::default()
        };
        let err = load_data_sources(&[ds], None, &limits).unwrap_err();
        assert!(format!("{err}").contains("max_file_bytes"), "got: {err}");
    }

    #[test]
    fn too_many_entries_rejected() {
        let content = "key,value\na,1\nb,2\nc,3\n";
        let path = temp_csv("entries", content);
        let ds = csv_source("t", path.display().to_string());
        let limits = DataSourceLimits {
            max_entries: 2,
            ..DataSourceLimits::default()
        };
        let err = load_data_sources(&[ds], None, &limits).unwrap_err();
        assert!(format!("{err}").contains("max_entries"), "got: {err}");
    }

    #[test]
    fn oversize_key_and_value_rejected() {
        let path = temp_csv("cell", "key,value\naaaaaaaa,1\n");
        let ds = csv_source("t", path.display().to_string());
        let key_limits = DataSourceLimits {
            max_key_bytes: 4,
            ..DataSourceLimits::default()
        };
        let err = load_data_sources(std::slice::from_ref(&ds), None, &key_limits).unwrap_err();
        assert!(format!("{err}").contains("max_key_bytes"), "got: {err}");

        let path2 = temp_csv("cell2", "key,value\na,11111111\n");
        let ds2 = csv_source("t", path2.display().to_string());
        let val_limits = DataSourceLimits {
            max_value_bytes: 4,
            ..DataSourceLimits::default()
        };
        let err2 = load_data_sources(&[ds2], None, &val_limits).unwrap_err();
        assert!(format!("{err2}").contains("max_value_bytes"), "got: {err2}");
    }

    #[test]
    fn too_many_tables_rejected() {
        let path = temp_csv("tables", "key,value\na,1\n");
        let a = csv_source("a", path.display().to_string());
        let b = csv_source("b", path.display().to_string());
        let limits = DataSourceLimits {
            max_tables: 1,
            ..DataSourceLimits::default()
        };
        let err = load_data_sources(&[a, b], None, &limits).unwrap_err();
        assert!(format!("{err}").contains("max_tables"), "got: {err}");
    }

    #[test]
    fn aggregate_total_bytes_rejected() {
        let content = "key,value\na,1\nb,2\n"; // ~18 bytes
        let a = csv_source("a", temp_csv("agg-a", content).display().to_string());
        let b = csv_source("b", temp_csv("agg-b", content).display().to_string());
        let limits = DataSourceLimits {
            max_total_bytes: 20,
            ..DataSourceLimits::default()
        };
        let err = load_data_sources(&[a, b], None, &limits).unwrap_err();
        assert!(format!("{err}").contains("max_total_bytes"), "got: {err}");
    }

    #[test]
    fn per_entry_override_loosens_global() {
        let path = temp_csv("override", "key,value\naaaaaaaa,1\n");
        let mut ds = csv_source("t", path.display().to_string());
        // Global key cap would reject an 8-byte key; per-entry override loosens it.
        ds.max_key_bytes = Some(64);
        let limits = DataSourceLimits {
            max_key_bytes: 4,
            ..DataSourceLimits::default()
        };
        let store = load_data_sources(&[ds], None, &limits).unwrap();
        assert_eq!(store.lookup("t", "aaaaaaaa"), "1");
    }

    #[test]
    fn per_entry_override_tightens_global() {
        let path = temp_csv("tighten", "key,value\na,1\nb,2\nc,3\n");
        let mut ds = csv_source("t", path.display().to_string());
        ds.max_entries = Some(2);
        // Global is generous; per-entry override is the binding constraint.
        let err = load_data_sources(&[ds], None, &DataSourceLimits::default()).unwrap_err();
        assert!(format!("{err}").contains("max_entries"), "got: {err}");
    }
}
