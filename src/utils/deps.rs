use std::process::Command;

/// Status of required and optional dependencies
#[derive(Debug, Clone)]
pub struct DependencyStatus;

impl DependencyStatus {
    /// Whether everything needed to encode is present. `libvmaf` is not part
    /// of this; it covers quality verification only.
    pub fn check() -> bool {
        check_command("ffmpeg", &["-version"]) && check_command("ffprobe", &["-version"])
    }

    /// Whether this `FFmpeg` build can run VMAF verification.
    pub fn vmaf_available() -> bool {
        check_vmaf_available()
    }

    /// Whether this `FFmpeg` build can encode Opus. Like `libvmaf`, not part
    /// of [`DependencyStatus::check`].
    pub fn libopus_available() -> bool {
        check_encoder_available("libopus")
    }

    /// Whether this `FFmpeg` build has the named encoder, e.g. the video
    /// encoder the configuration selected.
    pub fn encoder_available(ffmpeg_name: &str) -> bool {
        check_encoder_available(ffmpeg_name)
    }
}

/// Check if a command is available
fn check_command(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Check whether `FFmpeg` was built with a given encoder
fn check_encoder_available(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .is_ok_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                // The encoder name is the second column, after the capability
                // flags. Descriptions are not matched.
                .any(|l| l.split_whitespace().nth(1) == Some(name))
        })
}

/// Check if VMAF is available in `FFmpeg`
fn check_vmaf_available() -> bool {
    check_filter_available("libvmaf")
}

/// Check whether `FFmpeg` was built with a given filter
fn check_filter_available(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-filters"])
        .output()
        .is_ok_and(|o| {
            // The filter name is the second column, after the capability flags.
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.split_whitespace().nth(1) == Some(name))
        })
}

/// One prerequisite and, when it is missing, how to install it here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependencyReport {
    /// Stable machine key: `ffmpeg`, `libsvtav1`, `libvmaf`, `libopus`,
    /// `libplacebo` or `makemkv`.
    pub key: &'static str,
    /// Whether the prerequisite is satisfied.
    pub installed: bool,
    /// Command that installs it on this machine, when a one-line install exists.
    pub command: Option<&'static str>,
}

/// Report every prerequisite, each with the install command for this machine.
pub fn dependency_report() -> Vec<DependencyReport> {
    let os = std::env::consts::OS;
    let pm = linux_package_manager();
    [
        ("ffmpeg", DependencyStatus::check()),
        (
            "libsvtav1",
            DependencyStatus::encoder_available("libsvtav1"),
        ),
        ("libvmaf", DependencyStatus::vmaf_available()),
        ("libopus", DependencyStatus::libopus_available()),
        ("libplacebo", check_filter_available("libplacebo")),
        ("makemkv", check_command("makemkvcon", &["--version"])),
    ]
    .into_iter()
    .map(|(key, installed)| DependencyReport {
        key,
        installed,
        command: install_command(key, os, pm),
    })
    .collect()
}

/// Which Linux package manager is available, as the `pm` argument of
/// [`install_command`]: `Some("apt")`, `Some("dnf")`, `Some("pacman")` or
/// `None` for anything else.
fn linux_package_manager() -> Option<&'static str> {
    if std::env::consts::OS != "linux" {
        return None;
    }
    ["apt", "dnf", "pacman"]
        .into_iter()
        .find(|pm| check_command(pm, &["--version"]))
}

/// Install command for prerequisite `key` on `os` (a `std::env::consts::OS`
/// value) with Linux package manager `pm`. `None` means there is no one-line
/// install on that platform.
fn install_command(key: &str, os: &str, pm: Option<&str>) -> Option<&'static str> {
    let ffmpeg_pkg = match (os, pm) {
        ("linux", Some("apt")) => "sudo apt install ffmpeg",
        ("linux", Some("dnf")) => "sudo dnf install ffmpeg",
        ("linux", Some("pacman")) => "sudo pacman -S ffmpeg",
        ("macos", _) => "brew install ffmpeg",
        ("windows", _) => "winget install Gyan.FFmpeg",
        _ => return None,
    };
    match key {
        // Every FFmpeg feature ships in the platform's one ffmpeg package,
        // except libplacebo in Homebrew builds.
        "libplacebo" if os == "macos" => None,
        "ffmpeg" | "libsvtav1" | "libvmaf" | "libopus" | "libplacebo" => Some(ffmpeg_pkg),
        "makemkv" => match (os, pm) {
            ("macos", _) => Some("brew install --cask makemkv"),
            ("windows", _) => Some("winget install GuinpinSoft.MakeMKV"),
            ("linux", Some("pacman")) => Some("yay -S makemkv"),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{dependency_report, install_command};

    #[test]
    fn every_prerequisite_is_reported_once() {
        let report = dependency_report();
        let mut keys: Vec<&str> = report.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), report.len());
        assert_eq!(
            keys,
            [
                "ffmpeg",
                "libopus",
                "libplacebo",
                "libsvtav1",
                "libvmaf",
                "makemkv"
            ]
        );
    }

    #[test]
    fn ffmpeg_has_an_install_command_on_every_supported_platform() {
        for (os, pm) in [
            ("linux", Some("apt")),
            ("linux", Some("dnf")),
            ("linux", Some("pacman")),
            ("macos", None),
            ("windows", None),
        ] {
            assert!(install_command("ffmpeg", os, pm).is_some(), "{os} {pm:?}");
        }
    }

    #[test]
    fn libplacebo_has_no_homebrew_install() {
        assert_eq!(install_command("libplacebo", "macos", None), None);
        assert!(install_command("libplacebo", "linux", Some("apt")).is_some());
    }

    #[test]
    fn makemkv_command_is_platform_specific() {
        assert!(install_command("makemkv", "macos", None).is_some());
        assert!(install_command("makemkv", "windows", None).is_some());
        assert_eq!(install_command("makemkv", "linux", Some("apt")), None);
    }

    #[test]
    fn unknown_platform_has_no_command() {
        assert_eq!(install_command("ffmpeg", "freebsd", None), None);
    }
}
