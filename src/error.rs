use std::path::PathBuf;

/// AV1Converter application error
#[derive(Debug)]
pub enum AppError {
    /// File I/O error
    Io {
        path: PathBuf,
        operation: &'static str,
        message: String,
    },

    /// Video analysis failed
    Analysis(String),

    /// Configuration error
    Config(String),

    /// VMAF calculation failed
    Vmaf(String),

    /// Required dependency missing
    DependencyMissing(String),

    /// JSON parsing error
    Parse { context: String, message: String },

    /// Command execution failed
    CommandExecution(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Io {
                path,
                operation,
                message,
            } => {
                write!(
                    f,
                    "I/O error during '{}' on '{}': {}",
                    operation,
                    path.display(),
                    message
                )
            }
            AppError::Analysis(msg) => write!(f, "Video analysis failed: {}", msg),
            AppError::Config(msg) => write!(f, "Configuration error: {}", msg),
            AppError::Vmaf(msg) => write!(f, "VMAF calculation failed: {}", msg),
            AppError::DependencyMissing(dep) => write!(f, "Required dependency missing: {}", dep),
            AppError::Parse { context, message } => {
                write!(f, "Parse error in {}: {}", context, message)
            }
            AppError::CommandExecution(msg) => write!(f, "Command execution failed: {}", msg),
        }
    }
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
