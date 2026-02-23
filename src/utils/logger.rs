use tracing_appender::non_blocking::WorkerGuard;

/// Initialize logging based on AV1_DEBUG environment variable
pub fn init_logging() -> Option<WorkerGuard> {
    if std::env::var("AV1_DEBUG").is_ok() {
        let log_dir = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
            .or_else(|| std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("av1converter");

        let _ = std::fs::create_dir_all(&log_dir);

        let file_appender = tracing_appender::rolling::daily(&log_dir, "av1converter.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::fmt()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::DEBUG.into()),
            )
            .init();

        tracing::info!("AV1 Converter logging initialized");
        Some(guard)
    } else {
        None
    }
}
