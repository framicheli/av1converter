//! Background daemon lifecycle: detaching from the terminal, the PID file,
//! and stopping a running instance.
//!
//! Daemonization is Unix-only; on other platforms use `--start-foreground`.
//! running in the foreground.

use std::fs::File;
#[cfg(unix)]
use std::io::Read;
use std::io::{self, Write};
use std::path::PathBuf;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// How long the parent waits for the detached child to bind its listener
/// before declaring startup failed.
#[cfg(unix)]
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `stop` waits for the daemon to exit. Covers the daemon's
/// `HTTP_JOIN_GRACE`, then its `SHUTDOWN_GRACE` for a rip, an encode and the
/// worker join, and the final queue save.
#[cfg(unix)]
const STOP_TIMEOUT: Duration = Duration::from_secs(45);
/// How long `lock_pid_file` waits out a lock held for a moment by a
/// `locked_pid` probe.
#[cfg(unix)]
const PID_LOCK_WAIT: Duration = Duration::from_secs(1);

/// Data directory for the PID file, queue and background log
/// (same location the debug logger uses).
pub fn data_dir() -> PathBuf {
    crate::config::env_dir("XDG_DATA_HOME")
        .or_else(|| crate::config::env_dir("HOME").map(|home| home.join(".local/share")))
        .or_else(|| crate::config::env_dir("LOCALAPPDATA"))
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
    contents.lines().next()?.trim().parse().ok()
}

#[cfg(not(unix))]
fn locked_pid(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Append the bound socket address as the second line of the PID file.
pub fn record_listen(listen: &str) -> io::Result<()> {
    append_listen(&pid_file(), listen)
}

fn append_listen(path: &std::path::Path, listen: &str) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    writeln!(file)?;
    file.write_all(listen.as_bytes())?;
    file.sync_all()
}

/// The socket address the running daemon bound, from the PID file.
pub fn running_listen() -> Option<std::net::SocketAddr> {
    running_pid()?;
    listen_in(&pid_file())
}

fn listen_in(path: &std::path::Path) -> Option<std::net::SocketAddr> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .nth(1)?
        .trim()
        .parse()
        .ok()
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

/// Lock the PID file at `path`. A lock won on an inode a departing daemon has
/// already unlinked is dropped and the path opened again. A lock held briefly
/// by a probe is waited out for up to [`PID_LOCK_WAIT`].
#[cfg(unix)]
fn lock_pid_file(path: &std::path::Path, pid: u32) -> io::Result<File> {
    use std::os::fd::AsRawFd;

    let deadline = Instant::now() + PID_LOCK_WAIT;
    loop {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let busy = io::Error::last_os_error()
                .raw_os_error()
                .is_some_and(|code| code == libc::EAGAIN || code == libc::EWOULDBLOCK);
            if busy && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "another daemon owns the PID file",
            ));
        }
        if !is_linked_at(&file, path)? {
            continue;
        }
        file.set_len(0)?;
        file.write_all(pid.to_string().as_bytes())?;
        file.sync_all()?;
        return Ok(file);
    }
}

/// Whether `file` is the inode currently linked at `path`.
#[cfg(unix)]
fn is_linked_at(file: &File, path: &std::path::Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let open = file.metadata()?;
    match std::fs::metadata(path) {
        Ok(linked) => Ok(open.ino() == linked.ino() && open.dev() == linked.dev()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
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
pub(crate) fn alive(pid: u32) -> bool {
    // Signal 0 performs the permission/existence check without signalling
    unsafe { libc::kill(pid.cast_signed(), 0) == 0 }
}

/// Whether `pid` is a running process with this executable's image name.
#[cfg(windows)]
pub(crate) fn alive(pid: u32) -> bool {
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
                .any(|(image, found)| image_matches(image, &expected_image) && found == pid)
        })
}

/// Whether an image name reported by `tasklist` names `expected`, ignoring
/// case. `tasklist` can cut long names; a cut name of at least 25 characters
/// that starts `expected` matches.
#[cfg(any(test, windows))]
fn image_matches(reported: &str, expected: &str) -> bool {
    let reported = reported.to_ascii_lowercase();
    let expected = expected.to_ascii_lowercase();
    reported == expected || (reported.chars().count() >= 25 && expected.starts_with(&reported))
}

#[cfg(all(not(unix), not(windows)))]
pub(crate) fn alive(_pid: u32) -> bool {
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

    // An exit before the listen address is recorded means startup failed
    // (disabled in config, port in use, …); the reason is in the log file.
    wait_for_listen(&mut child, &pid_file(), STARTUP_TIMEOUT)?;
    Ok(child.id())
}

/// Wait until `child` holds the PID lock at `path` and has recorded its bound
/// address there, failing if it exits first or `timeout` passes.
#[cfg(unix)]
fn wait_for_listen(
    child: &mut std::process::Child,
    path: &std::path::Path,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return Err(io::Error::other("daemon exited during startup"));
        }
        if locked_pid(path) == Some(child.id()) && listen_in(path).is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("daemon did not start listening in time"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
    #[test]
    fn a_cut_tasklist_image_name_still_names_this_executable() {
        let expected = "av1converter-8cdc7a6885f2f4cc.exe";
        assert!(super::image_matches(
            "AV1CONVERTER-8CDC7A6885F2F4CC.EXE",
            expected
        ));
        assert!(super::image_matches(
            "av1converter-8cdc7a6885f2f4cc.e",
            expected
        ));
        assert!(super::image_matches("av1converter.exe", "av1converter.exe"));
        assert!(!super::image_matches("av1converter.exe", expected));
        assert!(!super::image_matches("av1", expected));
        assert!(!super::image_matches(
            "other-8cdc7a6885f2f4cc.exe",
            expected
        ));
    }

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

    #[cfg(unix)]
    #[test]
    fn a_momentary_probe_lock_does_not_refuse_the_daemon() {
        use std::os::fd::AsRawFd;

        let path = std::env::temp_dir().join(format!("av1c_pid_probe_{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"").unwrap();
        let probe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert_eq!(unsafe { libc::flock(probe.as_raw_fd(), libc::LOCK_EX) }, 0);
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(probe);
        });

        let guard = lock_pid_file(&path, std::process::id()).unwrap();
        release.join().unwrap();
        let refused = lock_pid_file(&path, std::process::id()).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);

        drop(guard);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn a_pid_file_replaced_after_opening_is_not_the_linked_one() {
        let path = std::env::temp_dir().join(format!("av1c_pid_inode_{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let stale = lock_pid_file(&path, std::process::id()).unwrap();
        assert!(is_linked_at(&stale, &path).unwrap());
        std::fs::remove_file(&path).unwrap();
        assert!(!is_linked_at(&stale, &path).unwrap());
        drop(lock_pid_file(&path, std::process::id()).unwrap());
        assert!(!is_linked_at(&stale, &path).unwrap());
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn startup_waits_for_the_recorded_listen_address() {
        let path = std::env::temp_dir().join(format!("av1c_pid_startup_{}", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut exited = std::process::Command::new("sh")
            .args(["-c", "exit 1"])
            .spawn()
            .unwrap();
        assert!(wait_for_listen(&mut exited, &path, Duration::from_secs(5)).is_err());

        let mut running = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .unwrap();
        let _guard = lock_pid_file(&path, running.id()).unwrap();
        assert!(wait_for_listen(&mut running, &path, Duration::from_millis(300)).is_err());
        append_listen(&path, "127.0.0.1:9124").unwrap();
        assert!(wait_for_listen(&mut running, &path, Duration::from_secs(5)).is_ok());

        let _ = running.kill();
        let _ = running.wait();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_listen_address_rides_on_the_second_line() {
        let path = std::env::temp_dir().join(format!("av1c_pid_listen_{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _guard = lock_pid_file(&path, std::process::id()).unwrap();
        assert_eq!(listen_in(&path), None);
        append_listen(&path, "127.0.0.1:9123").unwrap();
        assert_eq!(locked_pid(&path), Some(std::process::id()));
        assert_eq!(listen_in(&path), Some("127.0.0.1:9123".parse().unwrap()));
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
