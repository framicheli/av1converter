use std::fmt::Display;

#[derive(Debug, Clone)]
pub enum AppError {
    CommandExecutionError { message: String },
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::CommandExecutionError { message } => {
                write!(f, "Error executing the shell command: {}", message)
            }
        }
    }
}
