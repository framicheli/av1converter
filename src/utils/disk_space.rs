use std::path::Path;

/// Get available disk space in bytes for the given path
#[cfg(unix)]
pub fn available_space(path: &Path) -> Option<u64> {
    use nix::sys::statvfs::statvfs;
    let stat = statvfs(path).ok()?;
    Some(stat.blocks_available() as u64 * stat.fragment_size() as u64)
}

#[cfg(not(unix))]
pub fn available_space(_path: &Path) -> Option<u64> {
    None
}

/// Check if there is enough disk space for an estimated output size
pub fn has_enough_space(path: &Path, required_bytes: u64) -> bool {
    available_space(path)
        .map(|available| available > required_bytes)
        .unwrap_or(true) // If we can't check, assume it's fine
}
