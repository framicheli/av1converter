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

/// Format a file size in human-readable form.
///
/// Units are binary (1024-based) and carry the matching IEC labels. The web UI
/// formats the sizes it computes itself the same way.
pub fn format_file_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    let b = u128::from(bytes);
    if bytes >= GIB {
        let h = b * 100 / u128::from(GIB);
        format!("{}.{:02} GiB", h / 100, h % 100)
    } else if bytes >= MIB {
        let h = b * 10 / u128::from(MIB);
        format!("{}.{} MiB", h / 10, h % 10)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::format_file_size;

    /// Divisors are 1024, and the labels say so.
    #[test]
    fn sizes_carry_binary_labels() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1 KiB");
        assert_eq!(format_file_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GiB");
        // 24.1 GB on the box is 22.44 GiB.
        assert_eq!(format_file_size(24_100_000_000), "22.44 GiB");
    }
}
