//! Config Module

use crate::analysis::AnalysisResult;
use crate::data::Resolution;
use crate::encoder::{AV1Encoder, get_hdr_params, get_quality_params};
use config::{Config, File};

/// Track selection configuration
#[derive(Debug, Clone, Default)]
pub struct TrackSelection {
    pub audio_tracks: Vec<usize>,
    pub subtitle_tracks: Vec<usize>,
}

impl TrackSelection {
    pub fn is_select_all(&self) -> bool {
        self.audio_tracks.is_empty() && self.subtitle_tracks.is_empty()
    }
}

/// Encoding options for quality control
#[derive(Debug, Clone, Default)]
pub struct EncoderOptions {
    /// VMAF quality threshold
    pub vmaf: Option<f64>,
    /// Color transfer from source (for HDR passthrough)
    pub color_transfer: Option<String>,
}

impl EncoderOptions {
    /// Options from video analysis
    pub fn from_analysis(analysis: &AnalysisResult) -> Self {
        let settings = Config::builder()
            .add_source(File::with_name("config.toml"))
            .add_source(config::Environment::with_prefix("APP"))
            .build()
            .unwrap();
        let vmaf: f64 = settings.get("vmaf").unwrap_or(90.0);
        Self {
            vmaf: Some(vmaf),
            color_transfer: analysis.color_transfer().map(|s| s.to_string()),
        }
    }

    pub fn ffmpeg_args(
        &self,
        input: &str,
        output: &str,
        track_selection: &TrackSelection,
        encoder: AV1Encoder,
        is_dolby_vision: bool,
    ) -> Vec<String> {
        let mut args = vec![
            "-y".to_string(),
            "-nostdin".to_string(),
            "-i".to_string(),
            input.to_string(),
            "-map".to_string(),
            "0:v:0".to_string(),
        ];

        // Add track mappings
        if track_selection.is_select_all() {
            // Copy all audio and subtitle tracks
            args.extend(["-map".to_string(), "0:a?".to_string()]);
            args.extend(["-map".to_string(), "0:s?".to_string()]);
        } else {
            // Map specific audio tracks
            for &track_idx in &track_selection.audio_tracks {
                args.extend(["-map".to_string(), format!("0:a:{}", track_idx)]);
            }
            // Map specific subtitle tracks
            for &track_idx in &track_selection.subtitle_tracks {
                args.extend(["-map".to_string(), format!("0:s:{}", track_idx)]);
            }
        }

        // Video codec settings
        args.extend(["-c:v".to_string(), encoder.ffmpeg_name().to_string()]);

        // Pixel format (10-bit)
        args.extend(["-pix_fmt".to_string(), "yuv420p10le".to_string()]);

        // Audio and subtitle copy
        args.extend([
            "-c:a".to_string(),
            "copy".to_string(),
            "-c:s".to_string(),
            "copy".to_string(),
        ]);

        let encoder_profile = EncoderProfile::default();
        args.extend(get_quality_params(encoder, encoder_profile));

        if is_dolby_vision {
            args.extend(crate::encoder::get_dv_to_hdr10_params());
        } else {
            let transfer = self.color_transfer.as_deref();
            args.extend(get_hdr_params(encoder_profile, transfer));
        }

        args.push(output.to_string());
        args
    }
}

/// Quality profile for encoding
#[derive(Debug, Clone, Copy, Default)]
pub enum EncoderProfile {
    #[default]
    HD1080p,
    HD1080pHDR,
    UHD2160p,
    UHD2160pHDR,
}

impl From<Resolution> for EncoderProfile {
    fn from(resolution: Resolution) -> Self {
        match resolution {
            Resolution::Unknown => EncoderProfile::HD1080p,
            Resolution::HD1080p => EncoderProfile::HD1080p,
            Resolution::HD1080pHDR => EncoderProfile::HD1080pHDR,
            Resolution::UHD2160p => EncoderProfile::UHD2160p,
            Resolution::UHD2160pHDR => EncoderProfile::UHD2160pHDR,
            Resolution::HD1080pDV => EncoderProfile::HD1080pHDR,
            Resolution::UHD2160pDV => EncoderProfile::UHD2160pHDR,
        }
    }
}

/// Encoder configuration
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// Selected encoder to use
    pub selected_encoder: AV1Encoder,
    /// VMAF quality threshold
    pub vmaf_threshold: Option<f64>,
}

impl EncoderConfig {
    pub fn new() -> Self {
        let available_encoders = detect_available_encoders();
        let selected_encoder = available_encoders
            .first()
            .copied()
            .unwrap_or(AV1Encoder::SvtAv1);
        Self {
            selected_encoder,
            vmaf_threshold: Some(90.0),
        }
    }
}

/// Progress callback type for encoding progress updates
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

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
