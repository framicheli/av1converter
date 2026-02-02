use std::process::Command;

/// Status of required and optional dependencies
#[derive(Debug, Clone)]
pub struct DependencyStatus {
    pub ffmpeg: bool,
    pub ffprobe: bool,
    pub ab_av1: bool,
    pub vmaf: bool,
}

impl DependencyStatus {
    /// Check all dependencies
    pub fn check() -> Self {
        Self {
            ffmpeg: check_command("ffmpeg", &["-version"]),
            ffprobe: check_command("ffprobe", &["-version"]),
            ab_av1: check_command("ab-av1", &["--version"]),
            vmaf: check_vmaf_available(),
        }
    }

    /// Check if the minimum required dependencies are available
    pub fn has_required(&self) -> bool {
        self.ffmpeg && self.ffprobe
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

/// Check if VMAF is available in FFmpeg
fn check_vmaf_available() -> bool {
    Command::new("ffmpeg")
        .args(["-filters"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("libvmaf"))
        .unwrap_or(false)
}
