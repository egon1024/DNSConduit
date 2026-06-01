use conduit_config::EffectiveConfig;
use conduit_core::{spawn_configurator, ConfiguratorState, RuntimeSnapshot, SnapshotStore};
use conduit_metrics::TracingHub;
use conduit_proto::config::Config;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn workspace_fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(path)
}

pub fn control_setup(
    file_cfg: Config,
    config_path: PathBuf,
    base_dir: Option<PathBuf>,
) -> (
    Arc<SnapshotStore>,
    Arc<Mutex<EffectiveConfig>>,
    conduit_core::ConfiguratorHandle,
    Arc<TracingHub>,
) {
    let snapshots = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        file_cfg.clone(),
    )));
    let tracing = Arc::new(TracingHub::from_config(&file_cfg));
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));
    let state = ConfiguratorState {
        config_path,
        base_dir,
    };
    let configurator = spawn_configurator(snapshots.clone(), effective.clone(), state);
    (snapshots, effective, configurator, tracing)
}

#[allow(dead_code)]
pub fn minimal_control_setup() -> (
    Arc<SnapshotStore>,
    Arc<Mutex<EffectiveConfig>>,
    conduit_core::ConfiguratorHandle,
    Arc<TracingHub>,
) {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = conduit_config::load_yaml(yaml).expect("parse");
    control_setup(
        file_cfg,
        workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(workspace_fixture("tests/fixtures/config")),
    )
}
