//! User-level autostart: a systemd user unit on Linux, a launchd agent on macOS.
//!
//! The unit/plist is generated at install time so `ExecStart` is whatever
//! binary ran `--install-service` (a cargo build or `/usr/bin/av1converter`).
//! Nothing is stored in `config.toml`; [`installed`] is the OS state.

use std::io;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Write;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};

/// Whether this platform can install a login service.
pub fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

/// Outcome of a successful [`install`].
pub struct InstallOutcome {
    /// Headless Linux without lingering: the unit dies at logout unless the
    /// user runs `loginctl enable-linger`.
    pub linger_hint: bool,
}

/// Whether a login service is installed (cheap: filesystem only, no subprocess).
pub fn installed() -> bool {
    #[cfg(target_os = "linux")]
    {
        enabled_link().is_some_and(|path| path.exists())
    }
    #[cfg(target_os = "macos")]
    {
        plist_path().is_some_and(|path| path.exists())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

/// Write the unit/plist, enable it, and start the daemon unless one is already
/// running (starting a second instance would fail the port bind and, with a
/// restart policy, loop).
pub fn install() -> io::Result<InstallOutcome> {
    let exe = current_exe()?;
    #[cfg(target_os = "linux")]
    {
        let path = std::env::var("PATH").unwrap_or_default();
        linux_install(&exe, &path)?;
        Ok(InstallOutcome {
            linger_hint: linger_hint(),
        })
    }
    #[cfg(target_os = "macos")]
    {
        macos_install(&exe)?;
        Ok(InstallOutcome { linger_hint: false })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = exe;
        Err(unsupported())
    }
}

/// Disable and stop the service, then remove its unit/plist.
pub fn uninstall() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_uninstall()
    }
    #[cfg(target_os = "macos")]
    {
        macos_uninstall()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok(())
    }
}

/// Remove autostart without stopping the daemon serving the current session.
pub fn uninstall_keep_running() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_uninstall_keep_running()
    }
    #[cfg(target_os = "macos")]
    {
        remove_plist()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::other("starting at login is only supported on Linux (systemd) and macOS")
}

fn current_exe() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

// ── systemd (Linux) ──────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
const UNIT_NAME: &str = "av1converter.service";

#[cfg(target_os = "linux")]
fn systemd_user_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|dir| dir.join("systemd/user"))
}

#[cfg(target_os = "linux")]
fn unit_path() -> Option<PathBuf> {
    systemd_user_dir().map(|dir| dir.join(UNIT_NAME))
}

#[cfg(target_os = "linux")]
fn enabled_link() -> Option<PathBuf> {
    systemd_user_dir().map(|dir| dir.join("default.target.wants").join(UNIT_NAME))
}

#[cfg(target_os = "linux")]
fn linux_install(exe: &Path, path: &str) -> io::Result<()> {
    let dest = unit_path().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "HOME / XDG_CONFIG_HOME is not set")
    })?;
    write_file(&dest, &systemd_unit(exe, path))?;
    systemctl(&["daemon-reload"])?;
    if super::lifecycle::running_pid().is_some() {
        systemctl(&["enable", UNIT_NAME])?;
    } else {
        systemctl(&["enable", "--now", UNIT_NAME])?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_uninstall() -> io::Result<()> {
    let _ = systemctl(&["disable", "--now", UNIT_NAME]);
    remove_unit()
}

#[cfg(target_os = "linux")]
fn linux_uninstall_keep_running() -> io::Result<()> {
    systemctl(&["disable", UNIT_NAME])?;
    remove_unit()
}

#[cfg(target_os = "linux")]
fn remove_unit() -> io::Result<()> {
    if let Some(path) = unit_path() {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    let _ = systemctl(&["daemon-reload"]);
    Ok(())
}

#[cfg(target_os = "linux")]
fn systemctl(args: &[&str]) -> io::Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                io::Error::other("systemctl not found; autostart needs a systemd user session")
            } else {
                e
            }
        })?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        Err(io::Error::other(if msg.is_empty() {
            format!("systemctl --user {} failed", args.join(" "))
        } else {
            msg.to_string()
        }))
    }
}

/// Headless and linger is off: the user unit will not survive logout.
#[cfg(target_os = "linux")]
fn linger_hint() -> bool {
    if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return false;
    }
    // SAFETY: getuid is always safe.
    let uid = unsafe { libc::getuid() };
    let output = Command::new("loginctl")
        .args(["show-user", &uid.to_string(), "-p", "Linger"])
        .stdin(Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            !stdout.contains("Linger=yes")
        }
        _ => true,
    }
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) fn systemd_unit(exe: &Path, path: &str) -> String {
    let exe = systemd_quote(&exe.to_string_lossy());
    let mut unit = format!(
        "[Unit]\n\
         Description=AV1Converter web UI daemon\n\
         \n\
         [Service]\n\
         ExecStart={exe} --start-foreground\n\
         Restart=on-abnormal\n\
         Nice=10\n"
    );
    if !path.is_empty() {
        let escaped = path
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%");
        unit.push_str("Environment=\"PATH=");
        unit.push_str(&escaped);
        unit.push_str("\"\n");
    }
    unit.push_str("\n[Install]\nWantedBy=default.target\n");
    unit
}

#[cfg(any(test, target_os = "linux"))]
fn systemd_quote(s: &str) -> String {
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b'+' | b':'))
    {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

// ── launchd (macOS) ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
const PLIST_LABEL: &str = "com.av1converter.daemon";

#[cfg(target_os = "macos")]
fn plist_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{PLIST_LABEL}.plist"))
    })
}

#[cfg(target_os = "macos")]
fn macos_install(exe: &Path) -> io::Result<()> {
    let dest =
        plist_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    write_file(&dest, &launchd_plist(exe))?;
    // SAFETY: getuid is always safe.
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    if super::lifecycle::running_pid().is_none() {
        let _ = launchctl(&["bootout", &format!("{domain}/{PLIST_LABEL}")]);
        launchctl(&["bootstrap", &domain, &dest.to_string_lossy()])?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_uninstall() -> io::Result<()> {
    // SAFETY: getuid is always safe.
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = launchctl(&["bootout", &format!("{domain}/{PLIST_LABEL}")]);
    remove_plist()
}

#[cfg(target_os = "macos")]
fn remove_plist() -> io::Result<()> {
    if let Some(path) = plist_path() {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> io::Result<()> {
    let output = Command::new("launchctl")
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        Err(io::Error::other(if msg.is_empty() {
            format!("launchctl {} failed", args.join(" "))
        } else {
            msg.to_string()
        }))
    }
}

#[cfg(any(test, target_os = "macos"))]
pub(crate) fn launchd_plist(exe: &Path) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>com.av1converter.daemon</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         \t\t<string>{exe}</string>\n\
         \t\t<string>--start-foreground</string>\n\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         \t<key>KeepAlive</key>\n\
         \t<dict>\n\
         \t\t<key>Crashed</key>\n\
         \t\t<true/>\n\
         \t</dict>\n\
         \t<key>Nice</key>\n\
         \t<integer>10</integer>\n\
         \t<key>ProcessType</key>\n\
         \t<string>Background</string>\n\
         </dict>\n\
         </plist>\n"
    )
}

#[cfg(any(test, target_os = "macos"))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_unit_runs_foreground_and_does_not_restart_on_exit_code() {
        let unit = systemd_unit(Path::new("/usr/bin/av1converter"), "/usr/bin");
        assert!(unit.contains("ExecStart=/usr/bin/av1converter --start-foreground"));
        assert!(unit.contains("Restart=on-abnormal"));
        assert!(unit.contains("Nice=10"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("Environment=\"PATH=/usr/bin\""));
    }

    #[test]
    fn systemd_unit_quotes_a_path_with_spaces() {
        let unit = systemd_unit(Path::new("/home/user/My Apps/av1converter"), "");
        assert!(unit.contains("ExecStart=\"/home/user/My Apps/av1converter\" --start-foreground"));
        assert!(!unit.contains("Environment="));
    }

    #[test]
    fn launchd_plist_starts_at_login_and_restarts_only_on_crash() {
        let plist = launchd_plist(Path::new("/usr/local/bin/av1converter"));
        assert!(plist.contains("<string>/usr/local/bin/av1converter</string>"));
        assert!(plist.contains("<string>--start-foreground</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>Crashed</key>"));
        assert!(!plist.contains("SuccessfulExit"));
    }

    #[test]
    fn launchd_plist_escapes_xml_in_the_binary_path() {
        let plist = launchd_plist(Path::new("/opt/foo&bar/av1converter"));
        assert!(plist.contains("/opt/foo&amp;bar/av1converter"));
        assert!(!plist.contains("/opt/foo&bar/"));
    }

    #[test]
    fn supported_matches_the_platforms_we_generate_units_for() {
        assert_eq!(
            supported(),
            cfg!(any(target_os = "linux", target_os = "macos"))
        );
    }
}
