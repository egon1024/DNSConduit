//! Serialize effective `Config` to YAML (spec §5.4).

use crate::error::ConfigError;
use crate::file::config_to_yaml;
use conduit_proto::config::Config;

pub fn export_yaml(cfg: &Config) -> Result<String, ConfigError> {
    let y = config_to_yaml(cfg)?;
    serde_yaml::to_string(&y).map_err(ConfigError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::load_yaml;

    #[test]
    fn export_omits_default_backend_weight() {
        let yaml_in = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml_in).unwrap();
        let yaml_out = export_yaml(&cfg).unwrap();
        assert!(!yaml_out.contains("weight:"));
        let cfg2 = load_yaml(&yaml_out).unwrap();
        assert_eq!(
            crate::effective_backend_weight(&cfg2.pools[0].backends[0]),
            crate::DEFAULT_BACKEND_WEIGHT
        );
    }

    #[test]
    fn yaml_roundtrip_preserves_schema_version() {
        let yaml_in = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml_in).unwrap();
        let yaml_out = export_yaml(&cfg).unwrap();
        let cfg2 = load_yaml(&yaml_out).unwrap();
        assert_eq!(cfg.schema_version, cfg2.schema_version);
        assert_eq!(
            cfg.listeners.as_ref().unwrap().threads,
            cfg2.listeners.as_ref().unwrap().threads
        );
    }

    #[test]
    fn export_sparse_omits_default_sections() {
        let yaml_in = include_str!("../../../tests/fixtures/config/minimal-sparse.yaml");
        let cfg = load_yaml(yaml_in).unwrap();
        assert!(cfg.control.is_none());
        let yaml_out = export_yaml(&cfg).unwrap();
        assert!(!yaml_out.contains("forward:"));
        assert!(!yaml_out.contains("orchestrator:"));
        assert!(!yaml_out.contains("events:"));
        assert!(!yaml_out.contains("rhai:"));
        assert!(!yaml_out.contains("control:"));
        let cfg2 = load_yaml(&yaml_out).unwrap();
        assert_eq!(cfg.forward, cfg2.forward);
        assert_eq!(cfg.orchestrator, cfg2.orchestrator);
        assert_eq!(cfg.events, cfg2.events);
        assert_eq!(cfg.rhai, cfg2.rhai);
        assert!(cfg2.control.is_none());
    }
}
