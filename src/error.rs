use std::path::PathBuf;

/// `AV1Converter` application error
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

    /// JSON parsing error
    Parse { context: String, message: String },

    /// Command execution failed
    CommandExecution(String),

    /// The operation was cancelled
    Cancelled,

    /// Analysis of one file panicked.
    AnalysisPanicked(String),

    /// An analysis worker thread panicked.
    AnalysisThreadPanicked,

    /// The file carries no video stream.
    NoVideoStream,
}

impl AppError {
    /// The error in the given language. Details coming from `FFmpeg`, the
    /// operating system or a parser stay in their own wording.
    pub fn message(&self, lang: crate::i18n::Language) -> String {
        use crate::i18n::{Msg, t};
        match self {
            AppError::Cancelled => t(lang, Msg::Cancelled).to_string(),
            AppError::Io {
                path,
                operation,
                message,
            } => t(lang, Msg::ErrIo)
                .replace("{operation}", operation)
                .replace("{path}", &path.display().to_string())
                .replace("{message}", message),
            AppError::Analysis(msg) => t(lang, Msg::ErrAnalysisFailed).replace("{message}", msg),
            AppError::Config(msg) => t(lang, Msg::ErrConfigFailed).replace("{message}", msg),
            AppError::Vmaf(msg) => t(lang, Msg::ErrVmafFailed).replace("{message}", msg),
            AppError::Parse { context, message } => t(lang, Msg::ErrParseFailed)
                .replace("{context}", context)
                .replace("{message}", message),
            AppError::CommandExecution(msg) => {
                t(lang, Msg::ErrCommandFailed).replace("{message}", msg)
            }
            AppError::AnalysisPanicked(path) => {
                t(lang, Msg::ErrAnalysisPanicked).replace("{path}", path)
            }
            AppError::AnalysisThreadPanicked => t(lang, Msg::ErrAnalysisThreadPanicked).to_string(),
            AppError::NoVideoStream => t(lang, Msg::ErrNoVideoStream).to_string(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Cancelled => write!(f, "Cancelled"),
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
            AppError::Analysis(msg) => write!(f, "Video analysis failed: {msg}"),
            AppError::Config(msg) => write!(f, "Configuration error: {msg}"),
            AppError::Vmaf(msg) => write!(f, "VMAF calculation failed: {msg}"),
            AppError::Parse { context, message } => {
                write!(f, "Parse error in {context}: {message}")
            }
            AppError::CommandExecution(msg) => write!(f, "Command execution failed: {msg}"),
            AppError::AnalysisPanicked(path) => write!(f, "Analysis crashed on {path}"),
            AppError::AnalysisThreadPanicked => write!(f, "The analysis thread crashed"),
            AppError::NoVideoStream => write!(f, "The file has no video stream"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io {
            path: PathBuf::new(),
            operation: "file operation",
            message: format!("{} ({})", err, err.kind()),
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
        AppError::Config(format!("Failed to parse TOML: {err}"))
    }
}

impl From<toml::ser::Error> for AppError {
    fn from(err: toml::ser::Error) -> Self {
        AppError::Config(format!("Failed to serialize TOML: {err}"))
    }
}
