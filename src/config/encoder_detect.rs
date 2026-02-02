use serde::{Deserialize, Serialize};

/// AV1 encoders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoder {
    /// NVIDIA NVENC
    Nvenc,
    /// Intel Quick Sync Video (Arc GPUs)
    Qsv,
    /// AMD AMF
    Amf,
    /// SVT-AV1 software encoder
    SvtAv1,
}

impl Encoder {
    /// FFmpeg encoder name
    pub fn ffmpeg_name(&self) -> &'static str {
        match self {
            Encoder::Nvenc => "av1_nvenc",
            Encoder::Qsv => "av1_qsv",
            Encoder::Amf => "av1_amf",
            Encoder::SvtAv1 => "libsvtav1",
        }
    }

    /// Display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Encoder::Nvenc => "NVENC (NVIDIA)",
            Encoder::Qsv => "Quick Sync (Intel)",
            Encoder::Amf => "AMF (AMD)",
            Encoder::SvtAv1 => "SVT-AV1 (Software)",
        }
    }
}

impl Default for Encoder {
    fn default() -> Self {
        detect_encoder()
    }
}

impl std::fmt::Display for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Detect available AV1 encoder
///
/// Priority: Hardware > Software (SVT-AV1)
pub fn detect_encoder() -> Encoder {
    // macOS: No hardware AV1 encoding support yet
    #[cfg(target_os = "macos")]
    {
        Encoder::SvtAv1
    }

    #[cfg(not(target_os = "macos"))]
    {
        if has_nvidia_av1() {
            Encoder::Nvenc
        } else if has_intel_av1() {
            Encoder::Qsv
        } else if has_amd_av1() {
            Encoder::Amf
        } else {
            Encoder::SvtAv1
        }
    }
}

// Hardware detection functions

#[cfg(not(target_os = "macos"))]
fn has_nvidia_av1() -> bool {
    let output = match Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };

    let gpu_name = String::from_utf8_lossy(&output.stdout).to_lowercase();

    // RTX 40/50 series and Ada Lovelace architecture support AV1 encoding
    ["rtx 40", "rtx 50", "ada", "l40", "l4"]
        .iter()
        .any(|p| gpu_name.contains(p))
}

#[cfg(not(target_os = "macos"))]
fn has_intel_av1() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for Intel Arc GPU
        if let Ok(output) = Command::new("lspci").output() {
            let lspci = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if lspci.contains("intel") && lspci.contains("arc") {
                return true;
            }
        }

        // Check VA-API for AV1 encode
        if let Ok(output) = Command::new("vainfo").output() {
            let vainfo = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if vainfo.contains("vaentrypointencslice") && vainfo.contains("av1") {
                return true;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("wmic")
            .args(["path", "win32_VideoController", "get", "name"])
            .output()
        {
            let gpu_info = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if gpu_info.contains("intel") && gpu_info.contains("arc") {
                return true;
            }
        }
    }

    false
}

#[cfg(not(target_os = "macos"))]
fn has_amd_av1() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for RDNA3 GPUs (RX 7000 series)
        if let Ok(output) = Command::new("lspci").output() {
            let lspci = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if lspci.contains("amd") || lspci.contains("radeon") {
                let rdna3 = ["navi 31", "navi 32", "navi 33", "rx 7"];
                if rdna3.iter().any(|p| lspci.contains(p)) {
                    return true;
                }
            }
        }

        // Check VA-API
        if let Ok(output) = Command::new("vainfo").output() {
            let vainfo = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if vainfo.contains("radeon")
                && vainfo.contains("vaentrypointencslice")
                && vainfo.contains("av1")
            {
                return true;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("wmic")
            .args(["path", "win32_VideoController", "get", "name"])
            .output()
        {
            let gpu_info = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if gpu_info.contains("rx 7") {
                return true;
            }
        }
    }

    false
}
