use std::process::Command;

/// Status of required and optional dependencies
#[derive(Debug, Clone)]
pub struct DependencyStatus;

impl DependencyStatus {
    /// Whether everything needed to encode is present. `libvmaf` is deliberately
    /// not part of this: it is only needed for quality verification, and a build
    /// without it works fine for anyone who does not use VMAF.
    pub fn check() -> bool {
        check_command("ffmpeg", &["-version"]) && check_command("ffprobe", &["-version"])
    }

    /// Whether this `FFmpeg` build can run VMAF verification.
    pub fn vmaf_available() -> bool {
        check_vmaf_available()
    }

    /// Whether this `FFmpeg` build can encode Opus. Like `libvmaf`, it is not
    /// part of [`DependencyStatus::check`]: copying audio works without it.
    pub fn libopus_available() -> bool {
        check_encoder_available("libopus")
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
        .ok()
        .is_some_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                // The encoder name is the second column, after the capability
                // flags; matching the whole output would also hit descriptions.
                .any(|l| l.split_whitespace().nth(1) == Some(name))
        })
}

/// Check if VMAF is available in `FFmpeg`
fn check_vmaf_available() -> bool {
    Command::new("ffmpeg")
        .args(["-filters"])
        .output()
        .ok()
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("libvmaf"))
}
