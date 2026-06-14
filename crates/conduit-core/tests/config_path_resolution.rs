//! Contract tests: filesystem paths in config resolve against the config file directory,
//! not the process working directory. Absolute paths are honored.

use conduit_config::{load_yaml, resolve_config_path};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_events::{compile_from_config as compile_events, parse_destination, Destination};
use conduit_proto::config::Config;
use conduit_script::compile_from_config as compile_scripts;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Restores the original working directory when dropped (for cwd isolation tests).
struct RestoreCwd(PathBuf);

impl RestoreCwd {
    fn new() -> Self {
        Self(std::env::current_dir().expect("current_dir"))
    }
}

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn workspace_fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(rel)
}

fn write_config_tree(base: &Path) {
    fs::create_dir_all(base.join("scripts")).unwrap();
    fs::create_dir_all(base.join("data")).unwrap();
    fs::create_dir_all(base.join("run")).unwrap();

    fs::write(
        base.join("scripts/policy.rhai"),
        "// config-relative rhai\n",
    )
    .unwrap();
    fs::write(base.join("data/table.csv"), "key,value\nk1,v1\n").unwrap();

    let yaml = r#"schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 10
  timeout_ms: 2000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 5000
  txn_table_capacity: 64
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: rhai-hook
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: scripts/policy.rhai
data_sources:
  - name: tbl
    type: csv
    path: data/table.csv
    key_column: key
    value_column: value
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks:
    - type: dnstap
      name: tap
      destinations:
        - "unix:run/dnstap.sock"
      emit:
        - query
"#;
    fs::write(base.join("conduit.yaml"), yaml).unwrap();
}

#[test]
fn relative_paths_join_config_directory() {
    let base = Path::new("/etc/conduit");
    assert_eq!(
        resolve_config_path(Some(base), "scripts/policy.rhai"),
        PathBuf::from("/etc/conduit/scripts/policy.rhai")
    );
    assert_eq!(
        resolve_config_path(Some(base), "/var/run/s.sock"),
        PathBuf::from("/var/run/s.sock")
    );
}

#[test]
fn rhai_compile_uses_config_dir_when_cwd_differs() {
    let root = TempDir::new().unwrap();
    let config_dir = root.path().join("conduit");
    let other_cwd = root.path().join("elsewhere");
    fs::create_dir_all(&other_cwd).unwrap();
    write_config_tree(&config_dir);

    let yaml = fs::read_to_string(config_dir.join("conduit.yaml")).unwrap();
    let cfg = load_yaml(&yaml).unwrap();
    let base = config_dir.as_path();

    let _cwd = RestoreCwd::new();
    std::env::set_current_dir(&other_cwd).unwrap();

    let compiled = compile_scripts(&cfg, Some(base)).expect("compile with config base_dir");
    assert_eq!(compiled.scripts.len(), 1);
    assert!(
        compiled.scripts[0].path.ends_with("scripts/policy.rhai"),
        "resolved script path: {}",
        compiled.scripts[0].path
    );

    let err = compile_scripts(&cfg, None).expect_err("without base_dir cwd cannot find script");
    assert!(err.to_string().contains("failed to read"), "{err}");
}

#[test]
fn data_sources_load_from_config_dir_when_cwd_differs() {
    let root = TempDir::new().unwrap();
    let config_dir = root.path().join("conduit");
    let other_cwd = root.path().join("elsewhere");
    fs::create_dir_all(&other_cwd).unwrap();
    write_config_tree(&config_dir);

    let yaml = fs::read_to_string(config_dir.join("conduit.yaml")).unwrap();
    let cfg = load_yaml(&yaml).unwrap();

    let _cwd = RestoreCwd::new();
    std::env::set_current_dir(&other_cwd).unwrap();

    let compiled = compile_scripts(&cfg, Some(config_dir.as_path())).unwrap();
    assert_eq!(compiled.data_sources.lookup("tbl", "k1"), "v1");

    let err = compile_scripts(&cfg, None).expect_err("cwd must not resolve csv");
    assert!(err.to_string().contains("failed to read"), "{err}");
}

#[test]
fn dnstap_unix_destination_resolves_against_config_dir() {
    let base = Path::new("/etc/conduit");
    let dest = parse_destination("unix:run/dnstap.sock", Some(base)).unwrap();
    assert_eq!(
        dest,
        Destination::Unix(PathBuf::from("/etc/conduit/run/dnstap.sock"))
    );

    let compiled = compile_events(
        &Config {
            schema_version: 1,
            events: Some(conduit_proto::EventsConfig {
                queue_depth: 64,
                drop_policy: "drop_oldest".into(),
                sinks: vec![conduit_proto::EventSink {
                    r#type: "dnstap".into(),
                    name: Some("tap".into()),
                    destinations: vec!["unix:run/dnstap.sock".into()],
                    emit: vec!["query".into()],
                    ..Default::default()
                }],
            }),
            ..Default::default()
        },
        Some(base),
    );
    assert_eq!(compiled.sinks.len(), 1);
    assert_eq!(
        compiled.sinks[0].destinations[0],
        Destination::Unix(PathBuf::from("/etc/conduit/run/dnstap.sock"))
    );
}

#[test]
fn snapshot_build_resolves_all_config_relative_paths() {
    let root = TempDir::new().unwrap();
    let config_dir = root.path().join("conduit");
    let other_cwd = root.path().join("elsewhere");
    fs::create_dir_all(&other_cwd).unwrap();
    write_config_tree(&config_dir);

    let yaml = fs::read_to_string(config_dir.join("conduit.yaml")).unwrap();
    let cfg = load_yaml(&yaml).unwrap();

    let _cwd = RestoreCwd::new();
    std::env::set_current_dir(&other_cwd).unwrap();

    let snap = RuntimeSnapshot::try_from_config_with_base(cfg, Some(config_dir.as_path())).unwrap();
    assert!(!snap.scripting.is_empty());
    assert!(snap.events.enabled);
    assert_eq!(snap.scripting.data_sources.lookup("tbl", "k1"), "v1");
}

#[test]
fn absolute_rhai_path_works_from_any_cwd() {
    let script = workspace_fixture("tests/fixtures/rhai/set-vip-pool.rhai");
    let abs = script.canonicalize().unwrap();

    let cfg = load_yaml(&format!(
        r#"schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: abs
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
        abs.display()
    ))
    .unwrap();

    let root = TempDir::new().unwrap();
    let other = root.path().join("nowhere");
    fs::create_dir_all(&other).unwrap();
    let _cwd = RestoreCwd::new();
    std::env::set_current_dir(&other).unwrap();

    let compiled = compile_scripts(&cfg, None).expect("absolute path ignores base_dir");
    assert_eq!(compiled.scripts.len(), 1);
    assert_eq!(compiled.scripts[0].path, abs.display().to_string());
}
