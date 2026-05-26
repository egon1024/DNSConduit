use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("data source '{name}': {message}")]
    DataSource { name: String, message: String },
    #[error("script '{path}': {message}")]
    Script { path: String, message: String },
    #[error("metric '{name}': {message}")]
    Metric { name: String, message: String },
    #[error("rule '{rule_id}': {message}")]
    Rule { rule_id: String, message: String },
}
