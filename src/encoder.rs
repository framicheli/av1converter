//! Encoder Module
//!
//! Implement Encoding operations

use crate::config::{EncoderOptions, EncoderProfile, ProgressCallback, TrackSelection};
use crate::data::Resolution;
use crate::vmaf::{VmafOptions, VmafResult, calculate_vmaf};
use std::fmt;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tracing::{info, warn};

/// AV1 encoders
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Get quality parameters for a specific encoder and profile
pub fn get_quality_params(encoder: AV1Encoder, profile: EncoderProfile) -> Vec<String> {
    match encoder {
        AV1Encoder::SvtAv1 => get_svtav1_params(profile),
        AV1Encoder::Nvenc => get_nvenc_params(profile),
        AV1Encoder::Qsv => get_qsv_params(profile),
        AV1Encoder::Amf => get_amf_params(profile),
    }
}

/// SVT-AV1 parameters
fn get_svtav1_params(profile: EncoderProfile) -> Vec<String> {
    // Base CRF and film-grain values based on profile
    let (crf, film_grain) = match profile {
        // SDR 1080p: Lower CRF for quality, no film-grain needed
        EncoderProfile::HD1080p => ("24", 0),
        // HDR 1080p: Slightly higher CRF, moderate film-grain for HDR artifacts
        EncoderProfile::HD1080pHDR => ("25", 4),
        // SDR 4K: Medium CRF, some film-grain helps at this resolution
        EncoderProfile::UHD2160p => ("25", 5),
        // HDR 4K: Most demanding - lowest CRF, higher film-grain
        EncoderProfile::UHD2160pHDR => ("24", 6),
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

/// NVENC parameters
fn get_nvenc_params(profile: EncoderProfile) -> Vec<String> {
    let (cq, lookahead) = match profile {
        EncoderProfile::HD1080p => ("24", "32"),
        EncoderProfile::HD1080pHDR => ("23", "32"),
        EncoderProfile::UHD2160p => ("25", "48"),
        EncoderProfile::UHD2160pHDR => ("22", "48"),
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
fn get_qsv_params(profile: EncoderProfile) -> Vec<String> {
    let quality = match profile {
        EncoderProfile::HD1080p => "24",
        EncoderProfile::HD1080pHDR => "23",
        EncoderProfile::UHD2160p => "25",
        EncoderProfile::UHD2160pHDR => "22",
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
fn get_amf_params(profile: EncoderProfile) -> Vec<String> {
    let quality = match profile {
        EncoderProfile::HD1080p => "24",
        EncoderProfile::HD1080pHDR => "23",
        EncoderProfile::UHD2160p => "25",
        EncoderProfile::UHD2160pHDR => "22",
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
pub fn get_hdr_params(profile: EncoderProfile, transfer: Option<&str>) -> Vec<String> {
    match profile {
        EncoderProfile::HD1080pHDR | EncoderProfile::UHD2160pHDR => {
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
pub fn get_dv_to_hdr10_params() -> Vec<String> {
    vec![
        "-vf".to_string(),
        "setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc=smpte2084".to_string(),
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "smpte2084".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
    ]
}

/// Check if a resolution represents Dolby Vision content
pub fn is_dolby_vision_resolution(resolution: &Resolution) -> bool {
    matches!(resolution, Resolution::HD1080pDV | Resolution::UHD2160pDV)
}

/// Get video duration in seconds using ffprobe
fn get_video_duration(input: &str) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            input,
        ])
        .output()
        .ok()?;

    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn cleanup_partial_file(path: &str) {
    let _ = std::fs::remove_file(path);
}

/// Encode video with optional VMAF quality check
#[allow(clippy::too_many_arguments)]
pub fn encode_video(
    input: &str,
    output: &str,
    resolution: Resolution,
    track_selection: &TrackSelection,
    encoder: AV1Encoder,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
    options: &EncoderOptions,
) -> EncodeResult {
    // Check if source is Dolby Vision
    let is_dolby_vision = crate::encoder::is_dolby_vision_resolution(&resolution);

    let enc_options = EncoderOptions::default();
    let mut args =
        enc_options.ffmpeg_args(input, output, track_selection, encoder, is_dolby_vision);

    // Get video duration for progress calculation
    let duration = get_video_duration(input).unwrap_or_else(|| {
        warn!("Could not determine duration for {}", input);
        0.0
    });

    // Use a file for progress output (avoids pipe buffering issues on macOS)
    let progress_file =
        std::env::temp_dir().join(format!("ffmpeg_progress_{}", std::process::id()));
    let progress_path = progress_file.to_string_lossy().to_string();

    // Create empty progress file
    if let Err(e) = std::fs::File::create(&progress_file) {
        return EncodeResult::Error(format!("Failed to create progress file: {}", e));
    }

    // Insert progress args at the beginning (after -y -nostdin)
    args.insert(2, "-progress".to_string());
    args.insert(3, progress_path.clone());

    info!("Starting encode: {} -> {}", input, output);

    let mut child = match Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&progress_file);
            return EncodeResult::Error(format!("Failed to start ffmpeg: {}", e));
        }
    };

    // Main loop: poll progress file and check for completion
    let result = run_encode_loop(
        &mut child,
        &progress_file,
        duration,
        progress_callback,
        cancel_flag,
        output,
    );

    // Clean up progress file
    let _ = std::fs::remove_file(&progress_file);

    // If encoding succeeded and VMAF is enabled, calculate quality score
    if matches!(result, EncodeResult::Success) {
        return run_vmaf_check(input, output, options);
    }

    result
}

/// Run the main encoding loop
fn run_encode_loop(
    child: &mut Child,
    progress_file: &Path,
    duration: f64,
    mut progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
    output: &str,
) -> EncodeResult {
    loop {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            kill_child(child);
            cleanup_partial_file(output);
            return EncodeResult::Cancelled;
        }

        // Read progress file and find the latest out_time_us value
        if let Ok(content) = std::fs::read_to_string(progress_file) {
            let mut latest_time_us: Option<f64> = None;
            for line in content.lines() {
                if let Some(value) = line.strip_prefix("out_time_us=")
                    && let Ok(time_us) = value.trim().parse::<f64>()
                    && time_us > 0.0
                {
                    latest_time_us = Some(time_us);
                }
            }

            if let Some(time_us) = latest_time_us {
                let time_secs = time_us / 1_000_000.0;
                if duration > 0.0 {
                    let progress = (time_secs / duration * 100.0).min(100.0) as f32;
                    if let Some(ref mut cb) = progress_callback {
                        cb(progress);
                    }
                }
            }
        }

        // Check if ffmpeg has finished
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    // Try to get stderr for error details
                    let stderr = child
                        .stderr
                        .take()
                        .and_then(|mut s| {
                            use std::io::Read;
                            let mut buf = String::new();
                            s.read_to_string(&mut buf).ok()?;
                            Some(buf)
                        })
                        .unwrap_or_default();

                    cleanup_partial_file(output);

                    let error_msg = if stderr.is_empty() {
                        format!("ffmpeg failed with status: {}", status)
                    } else {
                        // Extract last few lines of stderr for error message
                        let last_lines: Vec<&str> = stderr.lines().rev().take(5).collect();
                        format!(
                            "ffmpeg failed ({}): {}",
                            status,
                            last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
                        )
                    };

                    return EncodeResult::Error(error_msg);
                }
                return EncodeResult::Success;
            }
            Ok(None) => {
                // Still running
                thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                return EncodeResult::Error(format!("Failed to check ffmpeg status: {}", e));
            }
        }
    }
}

/// Run VMAF quality check after encoding
fn run_vmaf_check(input: &str, output: &str, options: &EncoderOptions) -> EncodeResult {
    info!("Running VMAF quality check...");

    let input_path = Path::new(input);
    let output_path = Path::new(output);

    // Use quick mode for reasonable speed
    let vmaf_options = VmafOptions::quick();

    match calculate_vmaf(input_path, output_path, &vmaf_options) {
        Ok(vmaf) => {
            info!("VMAF score: {:.2} ({})", vmaf.score, vmaf.quality_grade());

            // Check against threshold if set
            if let Some(threshold) = options.vmaf
                && !vmaf.meets_threshold(threshold)
            {
                warn!(
                    "VMAF score {:.2} is below threshold {:.2}",
                    vmaf.score, threshold
                );

                return EncodeResult::QualityBelowThreshold { vmaf, threshold };
            }

            EncodeResult::SuccessWithVmaf(vmaf)
        }
        Err(e) => {
            warn!(
                "VMAF calculation failed: {}. Reporting success without score.",
                e
            );
            // VMAF failed but encoding succeeded, report success without score
            EncodeResult::Success
        }
    }
}

/// Result of encoding with optional VMAF score
#[derive(Debug)]
pub enum EncodeResult {
    /// Encoding completed successfully
    Success,
    /// Encoding completed with VMAF quality score
    SuccessWithVmaf(VmafResult),
    /// Encoding was cancelled by user
    Cancelled,
    /// Encoding failed with error message
    Error(String),
    /// Encoding succeeded but quality is below threshold
    QualityBelowThreshold { vmaf: VmafResult, threshold: f64 },
}
