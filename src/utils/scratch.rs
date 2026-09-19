//! A private directory for this process's temporary files.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::warn;

/// Private directory for progress files, ffmpeg stderr and VMAF logs, created
/// on first use and reused while it is still this user's private directory.
///
/// Created with `mkdir` under a random name in the system temp directory:
/// `mkdir` will not follow a final symlink and fails if the name is taken, so a
/// directory created this way is owned by nobody else, and the random name
/// cannot be occupied in advance. A directory that has been removed (macOS
/// clears old `$TMPDIR` entries) or replaced is never reused; a new one is
/// created under a fresh name. `None` means none could be created; callers
/// fail rather than fall back to the shared temp directory.
pub fn scratch_dir() -> Option<PathBuf> {
    static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut dir = DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = std::env::temp_dir();
    *dir = reuse_or_create(dir.take(), &base);
    if dir.is_none() {
        warn!(
            "Could not create a private scratch directory in {}",
            base.display()
        );
    }
    dir.clone()
}

/// `current` while it is still a private directory, else a new one in `base`.
fn reuse_or_create(current: Option<PathBuf>, base: &Path) -> Option<PathBuf> {
    if let Some(dir) = current.filter(|dir| is_private_dir(dir)) {
        return Some(dir);
    }
    for _ in 0..8 {
        let Ok(random) = super::random_hex(8) else {
            break;
        };
        let candidate = base.join(format!("av1converter-{random}"));
        if create_private_dir(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

/// A directory itself (not a symlink to one), owned by this user and closed to
/// group and others.
pub fn is_private_dir(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid is always safe.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return false;
        }
    }
    metadata.is_dir()
}

/// [`scratch_dir`] or a ready-made message explaining what went wrong.
pub fn scratch_path(name: &str) -> Result<PathBuf, String> {
    scratch_dir().map(|dir| dir.join(name)).ok_or_else(|| {
        format!(
            "Could not create a private temporary directory in {}",
            std::env::temp_dir().display()
        )
    })
}

/// Create `path` (and its parents) if missing, restricted to this user.
///
/// For directories that persist between runs — the data directory holding the
/// daemon log, the PID file and the debug log. The log appenders cannot set a
/// mode per file, so the restriction goes on the directory.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Create `path` as a fresh directory only this user can enter. Fails if the
/// name already exists in any form.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        // Set at creation, leaving no window at wider permissions.
        builder.mode(0o700);
    }
    builder.create(path)
}

// The directory itself is left behind at exit; the files inside are removed as
// each job finishes.

#[cfg(test)]
mod tests {
    use super::{create_private_dir, reuse_or_create, scratch_dir};

    /// Stable across calls, and actually a directory we can write into.
    #[test]
    fn scratch_dir_is_a_usable_private_directory() {
        let dir = scratch_dir().expect("a private scratch directory");
        assert_eq!(scratch_dir().as_ref(), Some(&dir));
        assert!(dir.is_dir());
        // Random, not derived from the PID: an attacker cannot occupy the name
        // in advance to force the "no private directory" path.
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(!name.contains(&std::process::id().to_string()));

        let file = dir.join("probe.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"x");
        let _ = std::fs::remove_file(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "no access for group or others");
        }
    }

    /// A scratch directory that was removed, or swapped for a symlink, is
    /// replaced by a new private one instead of being reused.
    #[test]
    fn a_missing_or_replaced_scratch_dir_is_recreated() {
        let base = std::env::temp_dir().join(format!("av1c_scratch_reuse_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let first = reuse_or_create(None, &base).unwrap();
        assert_eq!(
            reuse_or_create(Some(first.clone()), &base).as_ref(),
            Some(&first)
        );

        std::fs::remove_dir(&first).unwrap();
        let second = reuse_or_create(Some(first.clone()), &base).unwrap();
        assert_ne!(second, first);
        assert!(second.is_dir());

        #[cfg(unix)]
        {
            std::fs::remove_dir(&second).unwrap();
            let elsewhere = base.join("elsewhere");
            create_private_dir(&elsewhere).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &second).unwrap();
            let third = reuse_or_create(Some(second.clone()), &base).unwrap();
            assert_ne!(third, second);
            assert!(!std::fs::symlink_metadata(&third).unwrap().is_symlink());
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A name already taken is refused, including when taken by a symlink.
    #[test]
    fn an_occupied_name_is_refused() {
        let base = std::env::temp_dir().join(format!("av1c_scratch_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let target = base.join("real");
        std::fs::create_dir(&target).unwrap();
        let taken = base.join("taken");

        assert!(create_private_dir(&taken).is_ok());
        assert!(create_private_dir(&taken).is_err());

        #[cfg(unix)]
        {
            let link = base.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(create_private_dir(&link).is_err());
        }

        let _ = std::fs::remove_dir_all(&base);
    }
}
