//! Live child PIDs so a timed-out shutdown can kill ffmpeg / ffprobe /
//! makemkvcon instead of leaving them behind.

use std::collections::HashSet;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

static LIVE: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();

fn live() -> &'static Mutex<HashSet<u32>> {
    LIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashSet<u32>> {
    live()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Put the child in its own process group so [`kill_all`] can reach helpers.
pub fn configure(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

/// Unregisters `pid` when dropped, including on panic after spawn.
pub struct ChildGuard {
    pid: u32,
}

impl ChildGuard {
    pub fn register(pid: u32) -> Self {
        lock().insert(pid);
        Self { pid }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        lock().remove(&self.pid);
    }
}

/// SIGKILL the process group (Unix) or `taskkill /T /F` (Windows).
pub fn kill_all() {
    let pids: Vec<u32> = lock().drain().collect();
    for pid in pids {
        kill_pid(pid);
    }
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    // Negative pid: the group created by [`configure`].
    let _ = unsafe { libc::kill(-pid.cast_signed(), libc::SIGKILL) };
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

#[cfg(not(any(unix, windows)))]
fn kill_pid(_pid: u32) {}
