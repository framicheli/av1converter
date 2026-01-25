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
    /// Whether to run VMAF quality check after encoding (always true)
    pub run_vmaf: bool,
    /// VMAF quality threshold (default: 90.0)
    pub vmaf_threshold: Option<f64>,
}

/// Default VMAF quality threshold
pub const DEFAULT_VMAF_THRESHOLD: f64 = 90.0;

impl Default for EncoderConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EncoderConfig {
    /// Create a new encoder config by detecting available encoders
    /// VMAF is always enabled by default with a threshold of 90.0
    pub fn new() -> Self {
        let available_encoders = detect_available_encoders();
        let selected_encoder = select_encoder(&available_encoders);

        Self {
            selected_encoder,
            // VMAF is always enabled
            run_vmaf: true,
            vmaf_threshold: Some(DEFAULT_VMAF_THRESHOLD),
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
        "rtx 40", // RTX 40 series
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

/// Content type for optimized encoding parameters
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentType {
    /// Live action footage
    #[default]
    LiveAction,
    /// Animation / cartoon content
    Animation,
}

impl ContentType {
    /// Detect content type from filename (basic heuristic)
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        if lower.contains("anime")
            || lower.contains("animation")
            || lower.contains("cartoon")
            || lower.contains("animated")
        {
            ContentType::Animation
        } else {
            ContentType::LiveAction
        }
    }
}

/// Get quality parameters for a specific encoder and profile
pub fn get_quality_params(
    encoder: AV1Encoder,
    profile: QualityProfile,
    content_type: ContentType,
) -> Vec<String> {
    match encoder {
        AV1Encoder::SvtAv1 => get_svtav1_params(profile, content_type),
        AV1Encoder::Nvenc => get_nvenc_params(profile),
        AV1Encoder::Qsv => get_qsv_params(profile),
        AV1Encoder::Amf => get_amf_params(profile),
    }
}

/// SVT-AV1 parameters with optimized quality settings
///
/// Key improvements:
/// - Lower CRF values (24-26 instead of 28-30) for better quality
/// - Film-grain synthesis (4-8) to mask compression artifacts
/// - film-grain-denoise=1 to remove source grain and re-synthesize
/// - enable-overlays=1 for better handling of complex scenes
/// - scd=1 for improved scene change detection
fn get_svtav1_params(profile: QualityProfile, content_type: ContentType) -> Vec<String> {
    // Base CRF and film-grain values based on profile
    let (crf, base_film_grain) = match profile {
        // SDR 1080p: Lower CRF for quality, no film-grain needed
        QualityProfile::HD1080p => ("24", 0),
        // HDR 1080p: Slightly higher CRF, moderate film-grain for HDR artifacts
        QualityProfile::HD1080pHDR => ("25", 4),
        // SDR 4K: Medium CRF, some film-grain helps at this resolution
        QualityProfile::UHD2160p => ("25", 5),
        // HDR 4K: Most demanding - lowest CRF, higher film-grain
        QualityProfile::UHD2160pHDR => ("24", 6),
    };

    // Adjust film-grain based on content type
    let film_grain = match content_type {
        ContentType::Animation => 0, // Animation doesn't benefit from film-grain
        ContentType::LiveAction => base_film_grain,
    };

    // Build SVT-AV1 params string
    let svt_params = if film_grain > 0 {
        format!(
            "tune=0:film-grain={}:film-grain-denoise=1:enable-overlays=1:scd=1",
            film_grain
        )
    } else {
        // For animation: disable film-grain, enable temporal filtering
        "tune=0:film-grain=0:enable-overlays=1:scd=1:enable-tf=1".to_string()
    };

    vec![
        "-crf".to_string(),
        crf.to_string(),
        "-preset".to_string(),
        "4".to_string(), // Preset 4 = good speed/quality balance
        "-svtav1-params".to_string(),
        svt_params,
    ]
}

/// NVENC parameters with optimized quality settings
///
/// Key improvements:
/// - Lower CQ values (22-25 instead of 28-30)
/// - Preset p7 (slowest/best quality) instead of p4
/// - Multipass encoding for consistent quality
/// - Lookahead buffer for better rate control
/// - Spatial and temporal AQ for better bit distribution
fn get_nvenc_params(profile: QualityProfile) -> Vec<String> {
    let (cq, lookahead) = match profile {
        QualityProfile::HD1080p => ("24", "32"),
        QualityProfile::HD1080pHDR => ("23", "32"),
        QualityProfile::UHD2160p => ("25", "48"),
        QualityProfile::UHD2160pHDR => ("22", "48"),
    };

    vec![
        "-cq".to_string(),
        cq.to_string(),
        "-preset".to_string(),
        "p7".to_string(), // p7 = slowest/best quality
        "-tune".to_string(),
        "hq".to_string(),
        "-multipass".to_string(),
        "fullres".to_string(),
        "-rc-lookahead".to_string(),
        lookahead.to_string(),
        "-spatial-aq".to_string(),
        "1".to_string(),
        "-temporal-aq".to_string(),
        "1".to_string(),
    ]
}

/// Intel QSV parameters with optimized quality settings
fn get_qsv_params(profile: QualityProfile) -> Vec<String> {
    let quality = match profile {
        QualityProfile::HD1080p => "24",
        QualityProfile::HD1080pHDR => "23",
        QualityProfile::UHD2160p => "25",
        QualityProfile::UHD2160pHDR => "22",
    };

    vec![
        "-global_quality".to_string(),
        quality.to_string(),
        "-preset".to_string(),
        "veryslow".to_string(), // Best quality preset
        "-look_ahead".to_string(),
        "1".to_string(),
        "-look_ahead_depth".to_string(),
        "40".to_string(),
    ]
}

/// AMD AMF parameters with optimized quality settings
fn get_amf_params(profile: QualityProfile) -> Vec<String> {
    let quality = match profile {
        QualityProfile::HD1080p => "24",
        QualityProfile::HD1080pHDR => "23",
        QualityProfile::UHD2160p => "25",
        QualityProfile::UHD2160pHDR => "22",
    };

    vec![
        "-quality".to_string(),
        quality.to_string(),
        "-usage".to_string(),
        "transcoding".to_string(),
        "-rc".to_string(),
        "cqp".to_string(), // Constant QP mode for consistent quality
    ]
}

/// Get HDR color parameters with metadata passthrough
///
/// Key improvements:
/// - Proper color primaries, transfer, and space settings
/// - Metadata passthrough for mastering display info
/// - Support for both PQ (HDR10) and HLG transfer functions
pub fn get_hdr_params(profile: QualityProfile, transfer: Option<&str>) -> Vec<String> {
    match profile {
        QualityProfile::HD1080pHDR | QualityProfile::UHD2160pHDR => {
            // Determine transfer characteristic (PQ or HLG)
            let color_trc = match transfer {
                Some("arib-std-b67") => "arib-std-b67", // HLG
                _ => "smpte2084",                       // PQ (default for HDR)
            };

            vec![
                "-color_primaries".to_string(),
                "bt2020".to_string(),
                "-color_trc".to_string(),
                color_trc.to_string(),
                "-colorspace".to_string(),
                "bt2020nc".to_string(),
                // Copy metadata from source
                "-map_metadata".to_string(),
                "0".to_string(),
            ]
        }
        _ => vec![],
    }
}

/// Get parameters for converting Dolby Vision to HDR10
///
/// This extracts the HDR10 base layer from Dolby Vision content,
/// allowing playback on non-DV displays while preserving HDR.
/// The DV metadata is stripped but the HDR10 color information is retained.
pub fn get_dv_to_hdr10_params() -> Vec<String> {
    vec![
        // Set color parameters explicitly for HDR10
        "-vf".to_string(),
        "setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc=smpte2084".to_string(),
        // Ensure HDR10 color metadata in output
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "smpte2084".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
    ]
}

/// Check if a resolution represents Dolby Vision content
pub fn is_dolby_vision_resolution(resolution: &crate::analysis::Resolution) -> bool {
    matches!(
        resolution,
        crate::analysis::Resolution::HD1080pDV | crate::analysis::Resolution::UHD2160pDV
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_names() {
        assert_eq!(AV1Encoder::SvtAv1.ffmpeg_name(), "libsvtav1");
        assert_eq!(AV1Encoder::Nvenc.ffmpeg_name(), "av1_nvenc");
    }

    #[test]
    fn test_svtav1_params() {
        let params = get_svtav1_params(QualityProfile::HD1080p, ContentType::LiveAction);
        assert!(params.contains(&"-crf".to_string()));
        assert!(params.contains(&"24".to_string()));
    }

    #[test]
    fn test_content_type_detection() {
        assert_eq!(
            ContentType::from_filename("My_Anime_Episode_01.mkv"),
            ContentType::Animation
        );
        assert_eq!(
            ContentType::from_filename("Movie_2024.mkv"),
            ContentType::LiveAction
        );
    }

    #[test]
    fn test_animation_no_film_grain() {
        let params = get_svtav1_params(QualityProfile::HD1080p, ContentType::Animation);
        let svt_params = params
            .iter()
            .find(|p| p.contains("film-grain="))
            .expect("Should have film-grain param");
        assert!(svt_params.contains("film-grain=0"));
    }
}
