use std::process::Command;

/// Status of required and optional dependencies
#[derive(Debug, Clone)]
pub struct DependencyStatus;

impl DependencyStatus {
    /// Check all dependencies
    pub fn check() -> bool {
        check_command("ffmpeg", &["-version"])  // Check ffmpeg
            && check_command("ffprobe", &["-version"])  // Check ffprobe
            && check_vmaf_available() // Check libvmaf
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
