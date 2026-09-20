use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, InitError, RollingFileAppender, Rotation};

/// How many daily log files are kept, the current day included.
const MAX_LOG_FILES: usize = 7;

/// Environment variable [`crate::daemon::lifecycle::spawn_background`] sets on
/// the detached daemon, whose log goes to the rolling file rather than to the
/// redirected stdout.
pub const BACKGROUND_ENV: &str = "AV1_DAEMON_BACKGROUND";

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

/// A daily rolling appender writing `<prefix>.<date>` in `dir`, keeping the
/// last [`MAX_LOG_FILES`] files.
fn rolling_appender(dir: &Path, prefix: &str) -> Result<RollingFileAppender, InitError> {
    Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(prefix)
        .max_log_files(MAX_LOG_FILES)
        .build(dir)
}

/// Initialize logging for daemon mode: to the rolling `daemon.log.<date>` in
/// the data directory when the daemon runs detached, and to stdout for
/// `--start-foreground` and the systemd unit.
pub fn init_daemon_logging() -> Option<WorkerGuard> {
    if std::env::var_os(BACKGROUND_ENV).is_some() {
        return init_file_logging("daemon.log");
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(env_level().into()),
        )
        .init();
    None
}

/// Initialize logging to the rolling file in the data directory. The level is
/// INFO, or DEBUG when `AV1_DEBUG` selects it.
pub fn init_logging() -> Option<WorkerGuard> {
    init_file_logging("av1converter.log")
}

/// Initialize logging to `<prefix>.<date>` in the data directory.
fn init_file_logging(prefix: &str) -> Option<WorkerGuard> {
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

    let file_appender = match rolling_appender(&log_dir, prefix) {
        Ok(appender) => appender,
        Err(e) => {
            eprintln!("Could not open the log file in {}: {e}", log_dir.display());
            return None;
        }
    };
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
    use super::{MAX_LOG_FILES, level_for, rolling_appender};

    #[test]
    fn av1_debug_selects_debug_only_when_it_is_set_to_something_other_than_zero() {
        assert_eq!(level_for(None), tracing::Level::INFO);
        assert_eq!(level_for(Some("")), tracing::Level::INFO);
        assert_eq!(level_for(Some("0")), tracing::Level::INFO);
        assert_eq!(level_for(Some("1")), tracing::Level::DEBUG);
        assert_eq!(level_for(Some("yes")), tracing::Level::DEBUG);
    }

    #[test]
    fn daily_log_files_beyond_the_limit_are_pruned() {
        let dir = std::env::temp_dir().join(format!("av1c_log_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for day in 1..=10 {
            std::fs::write(dir.join(format!("test.log.2026-01-{day:02}")), b"old").unwrap();
        }
        std::fs::write(dir.join("keep-me.txt"), b"not a log").unwrap();

        let appender = rolling_appender(&dir, "test.log").unwrap();

        let logs = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("test.log"))
            .count();
        assert_eq!(logs, MAX_LOG_FILES);
        assert!(!dir.join("test.log.2026-01-01").exists());
        assert!(dir.join("test.log.2026-01-10").exists());
        assert!(dir.join("keep-me.txt").exists());

        drop(appender);
        let _ = std::fs::remove_dir_all(dir);
    }
}
