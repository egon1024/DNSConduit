use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
}
