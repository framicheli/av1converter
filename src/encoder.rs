use std::fmt;
#[cfg(not(target_os = "macos"))]
use std::process::Command;

/// AV1 encoders
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AV1Encoder {
    /// NVIDIA NVENC
    Nvenc,
    /// Intel Quick Sync Video
    Qsv,
    /// AMD AMF
    Amf,
    /// SVT-AV1 software encoder
    SvtAv1,
}

impl AV1Encoder {
    /// Get the ffmpeg encoder name
    pub fn ffmpeg_name(&self) -> &'static str {
        match self {
            AV1Encoder::Nvenc => "av1_nvenc",
            AV1Encoder::Qsv => "av1_qsv",
            AV1Encoder::Amf => "av1_amf",
            AV1Encoder::SvtAv1 => "libsvtav1",
        }
    }

    /// Get the encoder display name
    pub fn display_name(&self) -> &'static str {
        match self {
            AV1Encoder::Nvenc => "NVENC (NVIDIA)",
            AV1Encoder::Qsv => "Quick Sync (Intel)",
            AV1Encoder::Amf => "AMF (AMD)",
            AV1Encoder::SvtAv1 => "SVT-AV1 (Software)",
        }
    }
}

impl fmt::Display for AV1Encoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Encoder configuration
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// Selected encoder to use
    pub selected_encoder: AV1Encoder,
    /// Available encoders in the system
    #[allow(dead_code)]
    pub available_encoders: Vec<AV1Encoder>,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EncoderConfig {
    /// Create a new encoder config by detecting available encoders
    pub fn new() -> Self {
        let available_encoders = detect_available_encoders();
        let selected_encoder = select_encoder(&available_encoders);

        Self {
            selected_encoder,
            available_encoders,
        }
    }
}

/// Detect available AV1 encoders
pub fn detect_available_encoders() -> Vec<AV1Encoder> {
    // macOS: No AV1 hardware encoding available
    #[cfg(target_os = "macos")]
    {
        vec![AV1Encoder::SvtAv1]
    }

    // Linux/Windows: Check for hardware encoders
    #[cfg(not(target_os = "macos"))]
    {
        let mut encoders = Vec::new();

        // Check NVIDIA
        if has_nvidia_av1_support() {
            encoders.push(AV1Encoder::Nvenc);
        }

        // Check Intel Arc / QSV
        if has_intel_av1_support() {
            encoders.push(AV1Encoder::Qsv);
        }

        // Check AMD
        if has_amd_av1_support() {
            encoders.push(AV1Encoder::Amf);
        }

        // Software fallback
        encoders.push(AV1Encoder::SvtAv1);

        encoders
    }
}

/// Check NVIDIA GPU AV1 encoding support
#[cfg(not(target_os = "macos"))]
fn has_nvidia_av1_support() -> bool {
    let output = match Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };

    let gpu_name = String::from_utf8_lossy(&output.stdout).to_lowercase();

    let av1_capable_patterns = [
        "rtx 40", // RTX 50 series
        "rtx 50", // RTX 50 series
        "ada",    // Ada Lovelace architecture
        "l40",    // NVIDIA L40 data center GPU
        "l4",     // NVIDIA L4 data center GPU
    ];

    av1_capable_patterns
        .iter()
        .any(|pattern| gpu_name.contains(pattern))
}

/// Check Intel GPU AV1 encoding support
#[cfg(not(target_os = "macos"))]
fn has_intel_av1_support() -> bool {
    // Check for Intel Arc GPUs via lspci (Linux)
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("lspci").output() {
            let lspci_output = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if lspci_output.contains("intel") && lspci_output.contains("arc") {
                return true;
            }
        }

        if let Ok(output) = Command::new("vainfo").output() {
            let vainfo_output = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if vainfo_output.contains("vaentrypointencslice") && vainfo_output.contains("av1") {
                return true;
            }
        }
    }

    // Windows: Check for Intel Arc via WMIC or PowerShell
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

/// Check AMD GPU AV1 encoding support
#[cfg(not(target_os = "macos"))]
fn has_amd_av1_support() -> bool {
    // Check for AMD RDNA3 GPUs
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("lspci").output() {
            let lspci_output = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if lspci_output.contains("amd") || lspci_output.contains("radeon") {
                let rdna3_patterns = ["navi 31", "navi 32", "navi 33", "rx 7"];
                if rdna3_patterns.iter().any(|p| lspci_output.contains(p)) {
                    return true;
                }
            }
        }

        if let Ok(output) = Command::new("vainfo").output() {
            let vainfo_output = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if vainfo_output.contains("radeon")
                && vainfo_output.contains("vaentrypointencslice")
                && vainfo_output.contains("av1")
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

/// Select available encoder
/// uses hardware encoder if present or software as a fallback
fn select_encoder(available: &[AV1Encoder]) -> AV1Encoder {
    available.first().copied().unwrap_or(AV1Encoder::SvtAv1)
}

/// Quality profile for encoding
#[derive(Debug, Clone, Copy)]
pub enum QualityProfile {
    HD1080p,
    HD1080pHDR,
    UHD2160p,
    UHD2160pHDR,
}

/// Get quality parameters for a specific encoder and profile
pub fn get_quality_params(encoder: AV1Encoder, profile: QualityProfile) -> Vec<String> {
    match encoder {
        AV1Encoder::SvtAv1 => get_svtav1_params(profile),
        AV1Encoder::Nvenc => get_nvenc_params(profile),
        AV1Encoder::Qsv => get_qsv_params(profile),
        AV1Encoder::Amf => get_amf_params(profile),
    }
}

fn get_svtav1_params(profile: QualityProfile) -> Vec<String> {
    let (crf, film_grain) = match profile {
        QualityProfile::HD1080p => ("28", "0"),
        QualityProfile::HD1080pHDR => ("29", "1"),
        QualityProfile::UHD2160p => ("30", "1"),
        QualityProfile::UHD2160pHDR => ("30", "1"),
    };

    vec![
        "-crf".to_string(),
        crf.to_string(),
        "-preset".to_string(),
        "4".to_string(),
        "-svtav1-params".to_string(),
        format!("tune=0:film-grain={}", film_grain),
    ]
}

fn get_nvenc_params(profile: QualityProfile) -> Vec<String> {
    let cq = match profile {
        QualityProfile::HD1080p => "28",
        QualityProfile::HD1080pHDR => "29",
        QualityProfile::UHD2160p => "30",
        QualityProfile::UHD2160pHDR => "30",
    };

    vec![
        "-cq".to_string(),
        cq.to_string(),
        "-preset".to_string(),
        "p4".to_string(),
        "-tune".to_string(),
        "hq".to_string(),
    ]
}

fn get_qsv_params(profile: QualityProfile) -> Vec<String> {
    let quality = match profile {
        QualityProfile::HD1080p => "28",
        QualityProfile::HD1080pHDR => "29",
        QualityProfile::UHD2160p => "30",
        QualityProfile::UHD2160pHDR => "30",
    };

    vec![
        "-global_quality".to_string(),
        quality.to_string(),
        "-preset".to_string(),
        "medium".to_string(),
    ]
}

fn get_amf_params(profile: QualityProfile) -> Vec<String> {
    let quality = match profile {
        QualityProfile::HD1080p => "28",
        QualityProfile::HD1080pHDR => "29",
        QualityProfile::UHD2160p => "30",
        QualityProfile::UHD2160pHDR => "30",
    };

    vec![
        "-quality".to_string(),
        quality.to_string(),
        "-usage".to_string(),
        "transcoding".to_string(),
    ]
}

/// Get HDR color parameters for HDR profiles
pub fn get_hdr_params(profile: QualityProfile) -> Vec<String> {
    match profile {
        QualityProfile::HD1080pHDR | QualityProfile::UHD2160pHDR => vec![
            "-color_primaries".to_string(),
            "bt2020".to_string(),
            "-color_trc".to_string(),
            "smpte2084".to_string(),
            "-colorspace".to_string(),
            "bt2020nc".to_string(),
        ],
        _ => vec![],
    }
}
