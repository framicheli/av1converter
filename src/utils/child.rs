//! Live child PIDs of ffmpeg / ffprobe / makemkvcon, killed by a timed-out
//! shutdown.

use std::collections::HashMap;
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};

static LIVE: OnceLock<Mutex<HashMap<u32, Option<u64>>>> = OnceLock::new();

fn live() -> &'static Mutex<HashMap<u32, Option<u64>>> {
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<u32, Option<u64>>> {
    live()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Spawn `command` in its own process group and register it for
/// [`kill_all`]. The registry stays locked from spawn to registration.
pub fn spawn(command: &mut Command) -> std::io::Result<(Child, ChildGuard)> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut live = lock();
    let child = command.spawn()?;
    let pid = child.id();
    live.insert(pid, process_starttime(pid));
    Ok((child, ChildGuard { pid }))
}

/// Unregisters `pid` when dropped, including on panic after spawn.
pub struct ChildGuard {
    pid: u32,
}

impl ChildGuard {
    /// Drop the pid from the live set without waiting for `Drop`.
    pub fn unregister(pid: u32) {
        lock().remove(&pid);
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        lock().remove(&self.pid);
    }
}

/// SIGKILL the process group (Unix) or `taskkill /T /F` (Windows), then wait.
/// Unregisters the pid as soon as wait returns.
pub fn kill_and_wait(child: &mut Child) {
    let pid = child.id();
    kill_pid(pid);
    let _ = child.wait();
    ChildGuard::unregister(pid);
}

/// SIGKILL registered children. Skips a pid whose starttime no longer matches
/// the one recorded at registration (PID reuse after the child exited).
pub fn kill_all() {
    let targets: Vec<(u32, Option<u64>)> = lock().drain().collect();
    for (pid, registered) in targets {
        if let Some(expected) = registered {
            match process_starttime(pid) {
                // Readable and different: the pid was reused.
                Some(current) if current != expected => continue,
                // Unreadable /proc (hidepid, etc.): still try to kill.
                None | Some(_) => {}
            }
        } else if !pid_alive(pid) {
            continue;
        }
        kill_pid(pid);
    }
}

/// Whether a process with this pid currently exists. A pid that does not fit
/// `pid_t` counts as gone. Unlike daemon PID-file checks, this does **not**
/// require the image name to match this executable.
pub(crate) fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        // EPERM answers for a live process owned by someone else; only ESRCH
        // proves the pid is free.
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(windows)]
    {
        pid_exists_windows(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// True when Windows reports any process with this PID (any image name).
#[cfg(windows)]
fn pid_exists_windows(pid: u32) -> bool {
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
                    let _image = fields.next()?;
                    fields.next()?.parse::<u32>().ok()
                })
                .any(|found| found == pid)
        })
}

#[cfg(unix)]
pub(crate) fn kill_pid(pid: u32) {
    // Negative pid: the group created by [`configure`].
    let _ = unsafe { libc::kill(-pid.cast_signed(), libc::SIGKILL) };
}

#[cfg(windows)]
pub(crate) fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn kill_pid(_pid: u32) {}

/// Kernel starttime for `pid`, when the platform exposes one. Compared before
/// a kill to tell a reused pid from the original child.
fn process_starttime(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `comm` can contain spaces/parens; starttime is the field after the
        // closing paren of `(comm)`, counting from 1 as field 22 of the whole
        // line → index 19 among the space-split tokens after `)`.
        let after_comm = stat.rsplit_once(')')?.1;
        let starttime = after_comm.split_whitespace().nth(19)?;
        starttime.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn a_spawned_child_is_registered_when_spawn_returns() {
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        let (mut child, guard) = super::spawn(&mut command).unwrap();
        let registered = super::lock().contains_key(&child.id());
        super::kill_and_wait(&mut child);
        drop(guard);
        assert!(registered);
    }
}
