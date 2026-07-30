//! A private directory for this process's temporary files.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::warn;

/// Directory to put progress files, ffmpeg stderr and VMAF logs in.
///
/// These all have predictable names, and the system temp directory is usually
/// world-writable: creating them there directly lets anyone else on the machine
/// pre-plant a symlink under the name we are about to use and have us truncate
/// whatever it points at. `mkdir` refuses to follow a final symlink and fails
/// outright if the name is taken, so a directory we successfully created is one
/// nobody else owns — and every name inside it is then ours alone.
///
/// The name is random rather than derived from the PID, so it cannot be
/// occupied in advance to force a failure. `None` means no private directory
/// could be created at all; callers must fail rather than fall back to the
/// shared directory, which is the exact exposure this exists to close.
///
/// Created on first use and reused for the life of the process.
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

/// Create `path` (and its parents) if missing, and make sure only this user can
/// enter it.
///
/// For directories that legitimately persist between runs — the data directory
/// holding the daemon log, the PID file and the debug log. Those files record
/// every path the tool touches, and the log appenders offer no way to set a
/// mode per file, so the directory is where the restriction goes.
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
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        // Set at creation rather than after, so there is no window in which the
        // directory exists with wider permissions.
        builder.mode(0o700);
    }
    builder.create(path)
}

// ponytail: the directory itself is left behind at exit — the files inside are
// removed as each job finishes, and an empty directory in the system temp folder
// is what temp folders are for. Clean it up on shutdown if that ever bothers.

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

    /// A name already taken is refused rather than reused — including when it
    /// was taken by a symlink, which is the attack this exists to stop.
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
