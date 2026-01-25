use std::fmt;
use std::path::PathBuf;

/// Comprehensive error type for the AV1 converter application
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AppError {
    /// File I/O error
    Io {
        path: PathBuf,
        operation: &'static str,
        message: String,
    },

    /// VMAF calculation failed
    Vmaf { message: String },

    /// JSON parsing error
    Parse { context: String, message: String },

    /// Command execution failed
    CommandExecution { message: String },
}

impl std::error::Error for AppError {}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io {
                path,
                operation,
                message,
            } => {
                write!(
                    f,
                    "Failed to {} '{}': {}",
                    operation,
                    path.display(),
                    message
                )
            }
            AppError::Vmaf { message } => write!(f, "VMAF error: {}", message),
            AppError::Parse { context, message } => {
                write!(f, "Parse error in {}: {}", context, message)
            }
            AppError::CommandExecution { message } => {
                write!(f, "Error executing command: {}", message)
            }
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
