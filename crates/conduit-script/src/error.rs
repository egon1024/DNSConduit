use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("data source '{name}': {message}")]
    DataSource { name: String, message: String },
    #[error("script '{path}': {message}")]
    Script { path: String, message: String },
    #[error("metric '{name}': {message}")]
    Metric { name: String, message: String },
    #[error("rule '{rule_name}': {message}")]
    Rule { rule_name: String, message: String },
}
