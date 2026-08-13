//! A private directory for this process's temporary files.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::warn;

/// Private directory for progress files, ffmpeg stderr and VMAF logs, created
/// on first use and reused for the life of the process.
///
/// Created with `mkdir` under a random name in the system temp directory:
/// `mkdir` will not follow a final symlink and fails if the name is taken, so a
/// directory created this way is owned by nobody else, and the random name
/// cannot be occupied in advance. `None` means none could be created; callers
/// fail rather than fall back to the shared temp directory.
pub fn scratch_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::temp_dir();
        for _ in 0..8 {
            let Ok(random) = super::random_hex(8) else {
                break;
            };
            let candidate = base.join(format!("av1converter-{random}"));
            if create_private_dir(&candidate).is_ok() {
                return Some(candidate);
            }
        }
        warn!(
            "Could not create a private scratch directory in {}",
            base.display()
        );
        None
    })
    .as_deref()
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
    use super::{create_private_dir, scratch_dir};

    /// Stable across calls, and actually a directory we can write into.
    #[test]
    fn scratch_dir_is_a_usable_private_directory() {
        let dir = scratch_dir().expect("a private scratch directory");
        assert_eq!(Some(dir), scratch_dir());
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
