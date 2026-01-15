use std::path::PathBuf;
use thiserror::Error;

/// AV1Converter application error
#[derive(Debug, Error)]
pub enum AppError {
    /// File I/O error
    #[error("Failed to {operation} '{}': {message}", path.display())]
    Io {
        path: PathBuf,
        operation: &'static str,
        message: String,
    },

    /// Video analysis failed
    #[error("Analysis error: {0}")]
    Analysis(String),

    /// Configuration error
    #[error("Config error: {0}")]
    Config(String),

    /// VMAF calculation failed
    #[error("VMAF error: {0}")]
    Vmaf(String),

    /// Required dependency missing
    #[error("Missing dependency: {0}")]
    DependencyMissing(String),

    /// JSON parsing error
    #[error("Parse error in {context}: {message}")]
    Parse { context: String, message: String },

    /// Command execution failed
    #[error("Error executing command: {0}")]
    CommandExecution(String),
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io {
            path: PathBuf::new(),
            operation: "unknown",
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Parse {
            context: "JSON".to_string(),
            message: err.to_string(),
        }
    }
}

impl From<toml::de::Error> for AppError {
    fn from(err: toml::de::Error) -> Self {
        AppError::Config(format!("Failed to parse TOML: {}", err))
    }
}

impl From<toml::ser::Error> for AppError {
    fn from(err: toml::ser::Error) -> Self {
        AppError::Config(format!("Failed to serialize TOML: {}", err))
    }
}
