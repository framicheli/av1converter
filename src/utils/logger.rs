use tracing_appender::non_blocking::WorkerGuard;

/// The level `AV1_DEBUG` selects: DEBUG when it is set to a non-empty value
/// other than `0`, INFO otherwise.
fn level_for(av1_debug: Option<&str>) -> tracing::Level {
    match av1_debug {
        Some(value) if !value.is_empty() && value != "0" => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    }
}

/// The level the current environment selects.
fn env_level() -> tracing::Level {
    level_for(std::env::var("AV1_DEBUG").ok().as_deref())
}

/// Initialize logging for daemon mode: INFO to stdout, DEBUG when `AV1_DEBUG`
/// is set.
pub fn init_daemon_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(env_level().into()),
        )
        .init();
}

/// Initialize logging to the rolling file in the data directory. The level is
/// INFO, or DEBUG when `AV1_DEBUG` selects it.
pub fn init_logging() -> Option<WorkerGuard> {
    let log_dir = crate::daemon::lifecycle::data_dir();

    // The rolling appender names each file after the date and sets no
    // file mode; the restriction sits on the directory. The log records
    // the path of every file processed.
    if let Err(e) = super::ensure_private_dir(&log_dir) {
        eprintln!(
            "Could not prepare the log directory {}: {e}",
            log_dir.display()
        );
        return None;
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "av1converter.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(env_level().into()),
        )
        .init();

    tracing::info!("AV1 Converter logging initialized");
    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::level_for;

    #[test]
    fn av1_debug_selects_debug_only_when_it_is_set_to_something_other_than_zero() {
        assert_eq!(level_for(None), tracing::Level::INFO);
        assert_eq!(level_for(Some("")), tracing::Level::INFO);
        assert_eq!(level_for(Some("0")), tracing::Level::INFO);
        assert_eq!(level_for(Some("1")), tracing::Level::DEBUG);
        assert_eq!(level_for(Some("yes")), tracing::Level::DEBUG);
    }
}
