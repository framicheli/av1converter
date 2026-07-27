//! Background daemon lifecycle: detaching from the terminal, the PID file,
//! and stopping a running instance.
//!
//! Daemonization is Unix-only; on other platforms `--daemon` falls back to
//! running in the foreground.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long the parent waits for the detached child before declaring startup
/// failed (it exits immediately on e.g. a port already in use).
const SPAWN_GRACE: Duration = Duration::from_millis(600);
/// How long `stop` waits for the daemon to exit. Must exceed the daemon's
/// own `SHUTDOWN_GRACE` so a running encode can be cancelled cleanly.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// Data directory for the PID file and background log
/// (same location the debug logger uses).
fn data_dir() -> PathBuf {
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

/// PID recorded in the PID file, if that process is still alive.
/// A stale file (process gone) is treated as "not running".
pub fn running_pid() -> Option<u32> {
    let pid = std::fs::read_to_string(pid_file())
        .ok()?
        .trim()
        .parse()
        .ok()?;
    alive(pid).then_some(pid)
}

pub fn write_pid_file() -> io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::write(pid_file(), std::process::id().to_string())
}

pub fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_file());
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal 0 performs the permission/existence check without signalling
    unsafe { libc::kill(pid.cast_signed(), 0) == 0 }
}

/// Liveness cannot be probed without a signal API, so a recorded PID is taken
/// at face value: reporting "not running" for a daemon that is up would be
/// worse than occasionally trusting a stale PID file.
#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    true
}

/// Re-exec ourselves as `--daemon-foreground`, detached in a new session with
/// stdio redirected to [`log_file`]. Returns the daemon PID.
#[cfg(unix)]
pub fn spawn_background() -> io::Result<u32> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    std::fs::create_dir_all(data_dir())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())?;
    let log_err = log.try_clone()?;

    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--daemon-foreground")
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
        "background mode is only supported on Unix; use --daemon-foreground",
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
            remove_pid_file();
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
