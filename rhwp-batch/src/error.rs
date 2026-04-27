use thiserror::Error;

#[derive(Debug, Error)]
pub enum BatchError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HWP parse error: {0}")]
    Parse(String),
    #[error("HWP serialize error: {0}")]
    Serialize(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Template error: {0}")]
    Template(String),
    #[error("Template file not found: {0}")]
    TemplateNotFound(String),
    #[error("Output file already exists (use --overwrite): {0}")]
    OutputExists(String),
    #[error("Missing marker key '{0}'")]
    MissingKey(String),
}
