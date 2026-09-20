//! User-level autostart: a systemd user unit on Linux, a launchd agent on macOS.
//!
//! The unit/plist is generated at install time, with `ExecStart` naming the
//! binary that ran `--install-service` (a cargo build or
//! `/usr/bin/av1converter`).
//! Nothing is stored in `config.toml`; [`installed`] is the OS state.

use std::io;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Write;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
use std::time::{Duration, Instant};

/// How long an install waits for the started daemon to listen.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether this platform can install a login service.
pub fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

/// Outcome of a successful [`install`].
pub struct InstallOutcome {
    /// Headless Linux without lingering: the unit dies at logout unless the
    /// user runs `loginctl enable-linger`.
    pub linger_hint: bool,
    /// The started daemon was not listening yet when the install stopped
    /// waiting; the service stays installed.
    pub still_starting: bool,
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
/// running.
pub fn install() -> io::Result<InstallOutcome> {
    let exe = current_exe()?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let env = unit_env(|name| std::env::var(name).ok());
    #[cfg(target_os = "linux")]
    {
        let still_starting = linux_install(&exe, &env)?;
        Ok(InstallOutcome {
            linger_hint: linger_hint(),
            still_starting,
        })
    }
    #[cfg(target_os = "macos")]
    {
        let still_starting = macos_install(&exe, &env)?;
        Ok(InstallOutcome {
            linger_hint: false,
            still_starting,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = exe;
        Err(unsupported())
    }
}

/// Disable and stop the service, then remove its unit/plist.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(clippy::unnecessary_wraps)
)]
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
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(clippy::unnecessary_wraps)
)]
pub fn uninstall_keep_running() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_uninstall_keep_running()
    }
    #[cfg(target_os = "macos")]
    {
        macos_uninstall_keep_running()
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
    crate::config::env_dir("XDG_CONFIG_HOME")
        .or_else(|| crate::config::env_dir("HOME").map(|home| home.join(".config")))
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

/// Returns whether the started daemon was still coming up when the wait ended.
#[cfg(target_os = "linux")]
fn linux_install(exe: &Path, env: &[(&str, String)]) -> io::Result<bool> {
    let dest = unit_path().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "HOME / XDG_CONFIG_HOME is not set")
    })?;
    write_file(&dest, &systemd_unit(exe, env))?;
    systemctl(&["daemon-reload"])?;
    if super::lifecycle::running_pid().is_some() {
        systemctl(&["enable", UNIT_NAME])?;
        return Ok(false);
    }
    systemctl(&["enable", "--now", UNIT_NAME])?;
    // A failed start leaves no unit behind.
    let listening = wait_for_service_listen(
        || super::lifecycle::running_listen().is_some(),
        || {
            let state = systemctl(&["show", "-p", "ActiveState", "--value", UNIT_NAME])?;
            Ok(unit_starting_or_up(&state))
        },
        SERVICE_START_TIMEOUT,
        &format!("the service failed to start; see journalctl --user -u {UNIT_NAME}"),
    )
    .inspect_err(|_| {
        let _ = linux_uninstall();
    })?;
    Ok(!listening)
}

/// Wait until `listening` reports that the started daemon listens. Returns
/// `false` when `timeout` passes while `coming_up` still reports the service as
/// starting or running, and fails with `failed` once it does not.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
fn wait_for_service_listen(
    listening: impl Fn() -> bool,
    coming_up: impl Fn() -> io::Result<bool>,
    timeout: Duration,
    failed: &str,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if listening() {
            return Ok(true);
        }
        if !coming_up()? {
            return Err(io::Error::other(failed.to_string()));
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether a systemd `ActiveState` is on the way up or running.
#[cfg(any(test, target_os = "linux"))]
fn unit_starting_or_up(state: &str) -> bool {
    matches!(state.trim(), "activating" | "active" | "reloading")
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

/// Run `systemctl --user` and return its stdout.
#[cfg(target_os = "linux")]
fn systemctl(args: &[&str]) -> io::Result<String> {
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
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// The environment the login unit carries over from the installing shell:
/// `PATH` when it is not empty, and `XDG_CONFIG_HOME` and `XDG_DATA_HOME` when
/// they are absolute paths.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
fn unit_env(var: impl Fn(&str) -> Option<String>) -> Vec<(&'static str, String)> {
    let path = var("PATH")
        .filter(|path| !path.is_empty())
        .map(|path| ("PATH", path));
    let xdg = ["XDG_CONFIG_HOME", "XDG_DATA_HOME"]
        .into_iter()
        .filter_map(|name| {
            var(name)
                .filter(|dir| Path::new(dir).is_absolute())
                .map(|dir| (name, dir))
        });
    path.into_iter().chain(xdg).collect()
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) fn systemd_unit(exe: &Path, env: &[(&str, String)]) -> String {
    let exe = systemd_quote(&exe.to_string_lossy());
    let mut unit = format!(
        "[Unit]\n\
         Description=AV1Converter web UI daemon\n\
         \n\
         [Service]\n\
         ExecStart={exe} --start-foreground\n\
         Restart=on-abnormal\n\
         KillMode=mixed\n\
         TimeoutStopSec=45\n\
         Nice=10\n"
    );
    for (name, value) in env {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%");
        for part in ["Environment=\"", name, "=", &escaped, "\"\n"] {
            unit.push_str(part);
        }
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
        format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('%', "%%")
                .replace('$', "$$")
        )
    }
}

// ── launchd (macOS) ──────────────────────────────────────────────────────────

#[cfg(any(test, target_os = "macos"))]
const PLIST_LABEL: &str = "com.av1converter.daemon";

#[cfg(target_os = "macos")]
fn plist_path() -> Option<PathBuf> {
    crate::config::env_dir("HOME").map(|home| {
        home.join("Library/LaunchAgents")
            .join(format!("{PLIST_LABEL}.plist"))
    })
}

/// Returns whether the started daemon was still coming up when the wait ended.
#[cfg(target_os = "macos")]
fn macos_install(exe: &Path, env: &[(&str, String)]) -> io::Result<bool> {
    let dest =
        plist_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    write_file(&dest, &launchd_plist(exe, env))?;
    // SAFETY: getuid is always safe.
    let uid = unsafe { libc::getuid() };
    let domain = format!("gui/{uid}");
    let [verb, target] = launchd_toggle_args(true, uid);
    launchctl(&[&verb, &target])?;
    if super::lifecycle::running_pid().is_some() {
        return Ok(false);
    }
    let _ = launchctl(&["bootout", &format!("{domain}/{PLIST_LABEL}")]);
    launchctl(&["bootstrap", &domain, &dest.to_string_lossy()])?;
    // A failed start leaves no agent behind.
    let listening = wait_for_service_listen(
        || super::lifecycle::running_listen().is_some(),
        || {
            let print = launchctl(&["print", &format!("{domain}/{PLIST_LABEL}")])?;
            Ok(!launchd_job_exited(&print))
        },
        SERVICE_START_TIMEOUT,
        "the service failed to start; run av1converter --start-foreground to see why",
    )
    .inspect_err(|_| {
        let _ = macos_uninstall();
    })?;
    Ok(!listening)
}

/// Whether `launchctl print` output shows a job that has run and is no longer
/// running.
#[cfg(any(test, target_os = "macos"))]
fn launchd_job_exited(print: &str) -> bool {
    let field = |name: &str| {
        print
            .lines()
            .find_map(|line| line.trim().strip_prefix(name))
            .map(str::trim)
    };
    field("state = ") == Some("not running")
        && field("last exit code = ").is_some_and(|code| code != "(never exited)")
}

#[cfg(target_os = "macos")]
fn macos_uninstall() -> io::Result<()> {
    // SAFETY: getuid is always safe.
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = launchctl(&["bootout", &format!("{domain}/{PLIST_LABEL}")]);
    remove_plist()
}

#[cfg(target_os = "macos")]
fn macos_uninstall_keep_running() -> io::Result<()> {
    // SAFETY: getuid is always safe.
    let [verb, target] = launchd_toggle_args(false, unsafe { libc::getuid() });
    launchctl(&[&verb, &target])?;
    remove_plist()
}

/// `launchctl enable|disable gui/<uid>/<label>`. The override persists across
/// reboots; disabling leaves a running instance running and stops launchd from
/// relaunching or loading it.
#[cfg(any(test, target_os = "macos"))]
fn launchd_toggle_args(enable: bool, uid: u32) -> [String; 2] {
    [
        if enable { "enable" } else { "disable" }.to_string(),
        format!("gui/{uid}/{PLIST_LABEL}"),
    ]
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

/// Run `launchctl` and return its stdout.
#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> io::Result<String> {
    let output = Command::new("launchctl")
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
pub(crate) fn launchd_plist(exe: &Path, env: &[(&str, String)]) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let environment = if env.is_empty() {
        String::new()
    } else {
        let mut entries = String::from("\t<key>EnvironmentVariables</key>\n\t<dict>\n");
        for (name, value) in env {
            for part in [
                "\t\t<key>",
                name,
                "</key>\n\t\t<string>",
                &xml_escape(value),
                "</string>\n",
            ] {
                entries.push_str(part);
            }
        }
        entries.push_str("\t</dict>\n");
        entries
    };
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
         \t<string>Standard</string>\n\
         \t<key>ExitTimeOut</key>\n\
         \t<integer>45</integer>\n\
         {environment}\
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
        let unit = systemd_unit(
            Path::new("/usr/bin/av1converter"),
            &[("PATH", "/usr/bin".to_string())],
        );
        assert!(unit.contains("ExecStart=/usr/bin/av1converter --start-foreground"));
        assert!(unit.contains("Restart=on-abnormal"));
        assert!(unit.contains("KillMode=mixed"));
        assert!(unit.contains("TimeoutStopSec=45"));
        assert!(unit.contains("Nice=10"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("Environment=\"PATH=/usr/bin\""));
    }

    #[test]
    fn a_service_that_starts_listening_is_reported_up() {
        let checks = std::cell::Cell::new(0);
        let listening = || {
            checks.set(checks.get() + 1);
            checks.get() > 2
        };
        let up = super::wait_for_service_listen(
            listening,
            || Ok(true),
            std::time::Duration::from_secs(5),
            "failed",
        );
        assert!(up.unwrap());
    }

    #[test]
    fn a_slow_service_is_reported_as_still_starting() {
        let up = super::wait_for_service_listen(
            || false,
            || Ok(true),
            std::time::Duration::ZERO,
            "failed",
        );
        assert!(!up.unwrap());
    }

    #[test]
    fn a_service_that_stops_before_listening_is_a_failure() {
        let error = super::wait_for_service_listen(
            || false,
            || Ok(false),
            std::time::Duration::from_secs(5),
            "it stopped",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "it stopped");
    }

    #[test]
    fn a_launchd_job_counts_as_exited_only_after_it_ran_and_stopped() {
        let running = "com.av1converter.daemon = {\n\tstate = running\n\tpid = 3224\n\tlast exit code = (never exited)\n\t\tstate = active\n}";
        let waiting = "com.av1converter.daemon = {\n\tstate = not running\n\tlast exit code = (never exited)\n}";
        let failed = "com.av1converter.daemon = {\n\tstate = not running\n\truns = 1\n\tlast exit code = 1\n\t\tstate = active\n}";
        assert!(!launchd_job_exited(running));
        assert!(!launchd_job_exited(waiting));
        assert!(launchd_job_exited(failed));
    }

    #[test]
    fn only_a_starting_or_running_unit_counts_as_coming_up() {
        for state in ["activating", "active", "reloading", "active\n"] {
            assert!(unit_starting_or_up(state), "{state}");
        }
        for state in ["failed", "inactive", "deactivating", ""] {
            assert!(!unit_starting_or_up(state), "{state}");
        }
    }

    #[test]
    fn systemd_unit_quotes_a_path_with_spaces() {
        let unit = systemd_unit(Path::new("/home/user/My Apps/av1converter"), &[]);
        assert!(unit.contains("ExecStart=\"/home/user/My Apps/av1converter\" --start-foreground"));
        assert!(!unit.contains("Environment="));
    }

    #[test]
    fn systemd_unit_escapes_specifiers_and_variables_in_the_binary_path() {
        let unit = systemd_unit(Path::new("/opt/100%/$HOME/av1converter"), &[]);
        assert!(unit.contains("ExecStart=\"/opt/100%%/$$HOME/av1converter\" --start-foreground"));
    }

    #[test]
    fn launchd_plist_starts_at_login_and_restarts_only_on_crash() {
        let plist = launchd_plist(
            Path::new("/usr/local/bin/av1converter"),
            &[("PATH", "/opt/homebrew/bin:/usr/bin".to_string())],
        );
        assert!(plist.contains("<string>/usr/local/bin/av1converter</string>"));
        assert!(plist.contains(
            "<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>PATH</key>\n\t\t<string>/opt/homebrew/bin:/usr/bin</string>"
        ));
        assert!(plist.contains("<string>--start-foreground</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>Crashed</key>"));
        assert!(!plist.contains("SuccessfulExit"));
        assert!(plist.contains("<key>ExitTimeOut</key>\n\t<integer>45</integer>"));
    }

    #[test]
    fn launchd_autostart_toggles_the_users_gui_service() {
        assert_eq!(
            launchd_toggle_args(false, 501),
            ["disable", "gui/501/com.av1converter.daemon"]
        );
        assert_eq!(
            launchd_toggle_args(true, 501),
            ["enable", "gui/501/com.av1converter.daemon"]
        );
    }

    #[test]
    fn launchd_plist_escapes_xml_in_the_binary_path() {
        let plist = launchd_plist(
            Path::new("/opt/foo&bar/av1converter"),
            &[("PATH", "/opt/a&b/bin".to_string())],
        );
        assert!(plist.contains("/opt/foo&amp;bar/av1converter"));
        assert!(plist.contains("<string>/opt/a&amp;b/bin</string>"));
        assert!(
            !launchd_plist(Path::new("/bin/av1converter"), &[]).contains("EnvironmentVariables")
        );
        assert!(!plist.contains("/opt/foo&bar/"));
    }

    #[test]
    fn the_unit_carries_the_xdg_homes_of_the_installing_shell() {
        let env = unit_env(|name| match name {
            "PATH" => Some("/usr/bin".to_string()),
            "XDG_CONFIG_HOME" => Some("/srv/conf".to_string()),
            "XDG_DATA_HOME" => Some("relative/data".to_string()),
            _ => None,
        });
        assert_eq!(
            env,
            [
                ("PATH", "/usr/bin".to_string()),
                ("XDG_CONFIG_HOME", "/srv/conf".to_string()),
            ]
        );
        let unit = systemd_unit(Path::new("/usr/bin/av1converter"), &env);
        assert!(unit.contains("Environment=\"XDG_CONFIG_HOME=/srv/conf\"\n"));
        let plist = launchd_plist(Path::new("/usr/bin/av1converter"), &env);
        assert!(plist.contains(
            "\t\t<key>PATH</key>\n\t\t<string>/usr/bin</string>\n\t\t<key>XDG_CONFIG_HOME</key>\n\t\t<string>/srv/conf</string>\n\t</dict>"
        ));
    }

    #[test]
    fn supported_matches_the_platforms_we_generate_units_for() {
        assert_eq!(
            supported(),
            cfg!(any(target_os = "linux", target_os = "macos"))
        );
    }
}
