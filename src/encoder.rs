//! Encoder Module
//!
//! Handles AV1 video encoding with FFmpeg.

use crate::config::{Encoder, Profile};
use crate::vmaf::{VmafResult, calculate_vmaf};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

/// Progress callback type
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

/// Encoding result
#[derive(Debug)]
pub enum EncodeResult {
    /// Encoding completed successfully
    Success,
    /// Encoding completed with VMAF score
    SuccessWithVmaf(VmafResult),
    /// Encoding was cancelled
    Cancelled,
    /// Encoding failed
    Error(String),
    /// Quality below threshold
    QualityWarning { vmaf: VmafResult, threshold: f64 },
}

/// Track selection for encoding
#[derive(Debug, Clone, Default)]
pub struct TrackSelection {
    pub audio_indices: Vec<usize>,
    pub subtitle_indices: Vec<usize>,
}

/// Encode a video file to AV1
#[allow(clippy::too_many_arguments)]
pub fn encode(
    input: &str,
    output: &str,
    profile: Profile,
    tracks: &TrackSelection,
    encoder: Encoder,
    is_dolby_vision: bool,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
    vmaf_threshold: Option<f64>,
) -> EncodeResult {
    let args = build_ffmpeg_args(input, output, profile, tracks, encoder, is_dolby_vision);

    // Get video duration for progress calculation
    let duration = get_duration(input).unwrap_or(0.0);

    // Create progress file
    let progress_file =
        std::env::temp_dir().join(format!("ffmpeg_progress_{}", std::process::id()));
    if std::fs::File::create(&progress_file).is_err() {
        return EncodeResult::Error("Failed to create progress file".to_string());
    }

    // Insert progress args
    let mut args = args;
    args.insert(2, "-progress".to_string());
    args.insert(3, progress_file.to_string_lossy().to_string());

    info!("Encoding: {} -> {} with {}", input, output, encoder);

    // Start FFmpeg
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

    // Run encoding loop
    let result = run_encode_loop(
        &mut child,
        &progress_file,
        duration,
        progress_callback,
        cancel_flag,
        output,
    );

    // Cleanup
    let _ = std::fs::remove_file(&progress_file);

    // Run VMAF check if encoding succeeded
    if matches!(result, EncodeResult::Success) {
        return run_vmaf_check(input, output, vmaf_threshold);
    }

    result
}

/// Build FFmpeg arguments for encoding
fn build_ffmpeg_args(
    input: &str,
    output: &str,
    profile: Profile,
    tracks: &TrackSelection,
    encoder: Encoder,
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

    // Track mapping
    if tracks.audio_indices.is_empty() && tracks.subtitle_indices.is_empty() {
        // Copy all tracks
        args.extend(["-map".to_string(), "0:a?".to_string()]);
        args.extend(["-map".to_string(), "0:s?".to_string()]);
    } else {
        for idx in &tracks.audio_indices {
            args.extend(["-map".to_string(), format!("0:a:{}", idx)]);
        }
        for idx in &tracks.subtitle_indices {
            args.extend(["-map".to_string(), format!("0:s:{}", idx)]);
        }
    }

    // Video encoder
    args.extend(["-c:v".to_string(), encoder.ffmpeg_name().to_string()]);

    // 10-bit pixel format
    args.extend(["-pix_fmt".to_string(), "yuv420p10le".to_string()]);

    // Copy audio and subtitles
    args.extend([
        "-c:a".to_string(),
        "copy".to_string(),
        "-c:s".to_string(),
        "copy".to_string(),
    ]);

    // Encoder-specific quality parameters
    args.extend(get_quality_params(encoder, profile));

    // HDR/color parameters
    if is_dolby_vision {
        args.extend(get_dolby_vision_params());
    } else if profile.is_hdr() {
        args.extend(get_hdr_params());
    }

    args.push(output.to_string());
    args
}

/// Get encoder-specific quality parameters
fn get_quality_params(encoder: Encoder, profile: Profile) -> Vec<String> {
    match encoder {
        Encoder::SvtAv1 => get_svtav1_params(profile),
        Encoder::Nvenc => get_nvenc_params(profile),
        Encoder::Qsv => get_qsv_params(profile),
        Encoder::Amf => get_amf_params(profile),
    }
}

/// SVT-AV1 parameters
fn get_svtav1_params(profile: Profile) -> Vec<String> {
    let (crf, film_grain) = match profile {
        Profile::HD1080p => ("22", 0),
        Profile::HD1080pHDR => ("23", 3),
        Profile::UHD2160p => ("23", 4),
        Profile::UHD2160pHDR => ("22", 4),
    };

    let svt_params = if film_grain > 0 {
        format!(
            "tune=0:film-grain={}:film-grain-denoise=1:enable-overlays=1:scd=1",
            film_grain
        )
    } else {
        "tune=0:film-grain=0:enable-overlays=1:scd=1:enable-tf=1".to_string()
    };

    vec![
        "-crf".to_string(),
        crf.to_string(),
        "-preset".to_string(),
        "4".to_string(),
        "-svtav1-params".to_string(),
        svt_params,
    ]
}

/// NVENC parameters
fn get_nvenc_params(profile: Profile) -> Vec<String> {
    let (cq, lookahead) = match profile {
        Profile::HD1080p => ("24", "32"),
        Profile::HD1080pHDR => ("23", "32"),
        Profile::UHD2160p => ("25", "48"),
        Profile::UHD2160pHDR => ("22", "48"),
    };

    vec![
        "-cq".to_string(),
        cq.to_string(),
        "-preset".to_string(),
        "p7".to_string(),
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

/// Intel QSV parameters
fn get_qsv_params(profile: Profile) -> Vec<String> {
    let quality = match profile {
        Profile::HD1080p => "22",
        Profile::HD1080pHDR => "23",
        Profile::UHD2160p => "24",
        Profile::UHD2160pHDR => "22",
    };

    vec![
        "-global_quality".to_string(),
        quality.to_string(),
        "-preset".to_string(),
        "veryslow".to_string(),
        "-look_ahead".to_string(),
        "1".to_string(),
        "-look_ahead_depth".to_string(),
        "40".to_string(),
    ]
}

/// AMD AMF parameters
fn get_amf_params(profile: Profile) -> Vec<String> {
    let quality = match profile {
        Profile::HD1080p => "24",
        Profile::HD1080pHDR => "23",
        Profile::UHD2160p => "25",
        Profile::UHD2160pHDR => "22",
    };

    vec![
        "-quality".to_string(),
        quality.to_string(),
        "-usage".to_string(),
        "transcoding".to_string(),
        "-rc".to_string(),
        "cqp".to_string(),
    ]
}

/// HDR color parameters
fn get_hdr_params() -> Vec<String> {
    vec![
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "smpte2084".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
        "-map_metadata".to_string(),
        "0".to_string(),
    ]
}

/// Dolby Vision to HDR10 conversion parameters
fn get_dolby_vision_params() -> Vec<String> {
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

/// Get video duration in seconds
fn get_duration(input: &str) -> Option<f64> {
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

/// Run the encoding loop with progress updates
fn run_encode_loop(
    child: &mut Child,
    progress_file: &Path,
    duration: f64,
    mut progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
    output: &str,
) -> EncodeResult {
    loop {
        // Check cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(output);
            return EncodeResult::Cancelled;
        }

        // Read progress
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

        // Check if FFmpeg finished
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
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

                    let _ = std::fs::remove_file(output);

                    let error_msg = if stderr.is_empty() {
                        format!("ffmpeg failed with status: {}", status)
                    } else {
                        let last_lines: Vec<&str> = stderr.lines().rev().take(5).collect();
                        format!(
                            "ffmpeg failed: {}",
                            last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
                        )
                    };

                    return EncodeResult::Error(error_msg);
                }
                return EncodeResult::Success;
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                return EncodeResult::Error(format!("Failed to check ffmpeg status: {}", e));
            }
        }
    }
}

/// Run VMAF quality check after encoding
fn run_vmaf_check(input: &str, output: &str, threshold: Option<f64>) -> EncodeResult {
    info!("Running VMAF quality check...");

    let input_path = Path::new(input);
    let output_path = Path::new(output);

    match calculate_vmaf(input_path, output_path) {
        Ok(vmaf) => {
            info!("VMAF score: {:.2} ({})", vmaf.score, vmaf.quality_grade());

            if let Some(thresh) = threshold
                && !vmaf.meets_threshold(thresh)
            {
                warn!(
                    "VMAF score {:.2} is below threshold {:.2}",
                    vmaf.score, thresh
                );
                return EncodeResult::QualityWarning {
                    vmaf,
                    threshold: thresh,
                };
            }

            EncodeResult::SuccessWithVmaf(vmaf)
        }
        Err(e) => {
            warn!(
                "VMAF calculation failed: {}. Reporting success without score.",
                e
            );
            EncodeResult::Success
        }
    }
}
