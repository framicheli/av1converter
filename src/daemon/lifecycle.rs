//! Background daemon lifecycle: detaching from the terminal, the PID file,
//! and stopping a running instance.
//!
//! Daemonization is Unix-only; on other platforms use `--start-foreground`.
//! running in the foreground.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long the parent waits for the detached child before declaring startup
/// failed (it exits immediately on e.g. a port already in use).
const SPAWN_GRACE: Duration = Duration::from_millis(600);
/// How long `stop` waits for the daemon to exit. Must exceed the daemon's
/// own `SHUTDOWN_GRACE` so a running encode can be cancelled cleanly.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// Data directory for the PID file, queue and background log
/// (same location the debug logger uses).
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("av1converter")
}

pub fn pid_file() -> PathBuf {
    data_dir().join("daemon.pid")
}

pub fn log_file() -> PathBuf {
    data_dir().join("daemon.log")
}

/// The queue as it stood when the daemon last changed it, so a restart or a
/// crash does not lose what was waiting to encode.
pub fn queue_file() -> PathBuf {
    data_dir().join("queue.json")
}

/// PID recorded in the file while another process holds its daemon lock.
#[cfg(unix)]
fn locked_pid(path: &std::path::Path) -> Option<u32> {
    use std::os::fd::AsRawFd;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        return None;
    }
    let error = io::Error::last_os_error();
    if !error
        .raw_os_error()
        .is_some_and(|code| code == libc::EAGAIN || code == libc::EWOULDBLOCK)
    {
        return None;
    }
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    contents.trim().parse().ok()
}

#[cfg(not(unix))]
fn locked_pid(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// PID recorded in the locked PID file, if that process is still alive.
/// An unlocked file is stale even if its old PID has since been reused.
pub fn running_pid() -> Option<u32> {
    let path = pid_file();
    let pid = locked_pid(&path)?;
    if alive(pid) {
        Some(pid)
    } else {
        #[cfg(not(unix))]
        let _ = std::fs::remove_file(path);
        None
    }
}

#[cfg(unix)]
fn lock_pid_file(path: &std::path::Path, pid: u32) -> io::Result<File> {
    use std::os::fd::AsRawFd;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another daemon owns the PID file",
        ));
    }
    file.set_len(0)?;
    file.write_all(pid.to_string().as_bytes())?;
    file.sync_all()?;
    Ok(file)
}

#[cfg(not(unix))]
fn lock_pid_file(path: &std::path::Path, pid: u32) -> io::Result<File> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(pid.to_string().as_bytes())?;
    file.sync_all()?;
    Ok(file)
}

/// Owns the PID file for the lifetime of the daemon.
pub struct PidGuard {
    _file: File,
    path: PathBuf,
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Write and exclusively hold the PID file for the lifetime of the daemon.
pub fn write_pid_file() -> io::Result<PidGuard> {
    crate::utils::ensure_private_dir(&data_dir())?;
    let path = pid_file();
    let file = lock_pid_file(&path, std::process::id())?;
    Ok(PidGuard { _file: file, path })
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal 0 performs the permission/existence check without signalling
    unsafe { libc::kill(pid.cast_signed(), 0) == 0 }
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let expected_image = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| {
                    let mut fields = line.trim_matches('"').split("\",\"");
                    Some((fields.next()?, fields.next()?.parse::<u32>().ok()?))
                })
                .any(|(image, found)| image.eq_ignore_ascii_case(&expected_image) && found == pid)
        })
}

#[cfg(all(not(unix), not(windows)))]
fn alive(_pid: u32) -> bool {
    false
}

/// Re-exec ourselves as `--start-foreground`, detached in a new session with
/// stdio redirected to [`log_file`]. Returns the daemon PID.
#[cfg(unix)]
pub fn spawn_background() -> io::Result<u32> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    crate::utils::ensure_private_dir(&data_dir())?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    // The daemon logs the paths of everything it touches; that is the user's
    // business and nobody else's.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let log = options.open(log_file())?;
    let log_err = log.try_clone()?;

    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--start-foreground")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err)
        .current_dir("/");
    // New session: no controlling terminal, so closing the shell that
    // launched us cannot HUP the daemon.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;

    // An immediate exit means startup failed (disabled in config, port in
    // use, …); the reason is in the log file.
    std::thread::sleep(SPAWN_GRACE);
    if child.try_wait()?.is_some() {
        return Err(io::Error::other("daemon exited during startup"));
    }
    Ok(child.id())
}

#[cfg(not(unix))]
pub fn spawn_background() -> io::Result<u32> {
    Err(io::Error::other(
        "background mode is only supported on Unix; use --start-foreground",
    ))
}

/// Send SIGTERM and wait for the daemon to exit (it may spend up to its
/// shutdown grace period cancelling a running encode).
#[cfg(unix)]
pub fn stop(pid: u32) -> io::Result<()> {
    if unsafe { libc::kill(pid.cast_signed(), libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        if !alive(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::other("daemon did not exit in time"))
}

#[cfg(not(unix))]
pub fn stop(_pid: u32) -> io::Result<()> {
    Err(io::Error::other("--stop is only supported on Unix"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unlocked_pid_file_is_stale_even_for_a_live_pid() {
        let path = std::env::temp_dir().join(format!("av1c_pid_lock_{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let guard = lock_pid_file(&path, std::process::id()).unwrap();
        assert_eq!(locked_pid(&path), Some(std::process::id()));
        drop(guard);
        // A child forked concurrently by another test briefly inherits open
        // descriptors until exec applies O_CLOEXEC; wait out that window.
        let deadline = Instant::now() + Duration::from_secs(1);
        while locked_pid(&path).is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(locked_pid(&path), None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pid_guard_removes_its_file_on_drop() {
        let path = std::env::temp_dir().join(format!("av1c_pid_guard_{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let file = lock_pid_file(&path, std::process::id()).unwrap();
        let guard = PidGuard {
            _file: file,
            path: path.clone(),
        };
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }
}
