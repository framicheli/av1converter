use std::time::Duration;

/// Format a duration as HH:MM:SS or MM:SS
pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Format a file size in human-readable form
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    let b = u128::from(bytes);
    if bytes >= GB {
        let h = b * 100 / u128::from(GB);
        format!("{}.{:02} GB", h / 100, h % 100)
    } else if bytes >= MB {
        let h = b * 10 / u128::from(MB);
        format!("{}.{} MB", h / 10, h % 10)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}
