use crate::analysis::{AnalysisResult, Resolution};
use crate::encoder::{AV1Encoder, ContentType, QualityProfile, get_hdr_params, get_quality_params};
use crate::vmaf::{VmafOptions, VmafResult, calculate_vmaf};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum EncoderProfile {
    HD1080p,
    HD1080pHDR,
    UHD2160p,
    UHD2160pHDR,
}

impl From<Resolution> for EncoderProfile {
    fn from(resolution: Resolution) -> Self {
        match resolution {
            Resolution::HD1080p => EncoderProfile::HD1080p,
            Resolution::HD1080pHDR => EncoderProfile::HD1080pHDR,
            Resolution::UHD2160p => EncoderProfile::UHD2160p,
            Resolution::UHD2160pHDR => EncoderProfile::UHD2160pHDR,
            Resolution::HD1080pDV => EncoderProfile::HD1080pHDR,
            Resolution::UHD2160pDV => EncoderProfile::UHD2160pHDR,
        }
    }
}

impl From<EncoderProfile> for QualityProfile {
    fn from(profile: EncoderProfile) -> Self {
        match profile {
            EncoderProfile::HD1080p => QualityProfile::HD1080p,
            EncoderProfile::HD1080pHDR => QualityProfile::HD1080pHDR,
            EncoderProfile::UHD2160p => QualityProfile::UHD2160p,
            EncoderProfile::UHD2160pHDR => QualityProfile::UHD2160pHDR,
        }
    }
}

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
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    /// Run VMAF quality check after encoding
    pub run_vmaf: bool,
    /// VMAF quality threshold
    pub vmaf_threshold: Option<f64>,
    /// Keep the encoded file even if quality is below threshold
    pub keep_low_quality: bool,
    /// Content type for optimized parameters
    pub content_type: ContentType,
    /// Color transfer from source (for HDR passthrough)
    pub color_transfer: Option<String>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            run_vmaf: false,
            vmaf_threshold: None,
            keep_low_quality: true,
            content_type: ContentType::default(),
            color_transfer: None,
        }
    }
}

impl EncodeOptions {
    /// Options from video analysis
    pub fn from_analysis(analysis: &AnalysisResult, filename: &str) -> Self {
        Self {
            run_vmaf: false,
            vmaf_threshold: None,
            keep_low_quality: true,
            content_type: ContentType::from_filename(filename),
            color_transfer: analysis.color_transfer().map(|s| s.to_string()),
        }
    }

    /// Enable VMAF quality checking with optional threshold
    #[cfg(test)]
    pub fn with_vmaf(mut self, threshold: Option<f64>) -> Self {
        self.run_vmaf = true;
        self.vmaf_threshold = threshold;
        self
    }
}

impl EncoderProfile {
    pub fn build_ffmpeg_args(
        &self,
        input: &str,
        output: &str,
        track_selection: &TrackSelection,
        encoder: AV1Encoder,
        options: &EncodeOptions,
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

        // Get quality parameters for the specific encoder and profile
        let quality_profile: QualityProfile = (*self).into();
        args.extend(get_quality_params(
            encoder,
            quality_profile,
            options.content_type,
        ));

        if is_dolby_vision {
            args.extend(crate::encoder::get_dv_to_hdr10_params());
        } else {
            let transfer = options.color_transfer.as_deref();
            args.extend(get_hdr_params(quality_profile, transfer));
        }

        args.push(output.to_string());
        args
    }
}

/// Progress callback type for encoding progress updates
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

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
    options: &EncodeOptions,
) -> EncodeResult {
    // Check if source is Dolby Vision (needs conversion to HDR10)
    let is_dolby_vision = crate::encoder::is_dolby_vision_resolution(&resolution);

    let profile: EncoderProfile = resolution.into();
    let mut args = profile.build_ffmpeg_args(
        input,
        output,
        track_selection,
        encoder,
        options,
        is_dolby_vision,
    );

    // Get video duration for progress calculation
    let duration = get_video_duration(input).unwrap_or_else(|| {
        tracing::warn!("Could not determine duration for {}", input);
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

    tracing::info!("Starting encode: {} -> {}", input, output);
    tracing::debug!("FFmpeg args: {:?}", args);

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
    if matches!(result, EncodeResult::Success) && options.run_vmaf {
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
fn run_vmaf_check(input: &str, output: &str, options: &EncodeOptions) -> EncodeResult {
    tracing::info!("Running VMAF quality check...");

    let input_path = Path::new(input);
    let output_path = Path::new(output);

    // Use quick mode for reasonable speed
    let vmaf_options = VmafOptions::quick();

    match calculate_vmaf(input_path, output_path, &vmaf_options) {
        Ok(vmaf) => {
            tracing::info!("VMAF score: {:.2} ({})", vmaf.score, vmaf.quality_grade());

            // Check against threshold if set
            if let Some(threshold) = options.vmaf_threshold
                && !vmaf.meets_threshold(threshold)
            {
                tracing::warn!(
                    "VMAF score {:.2} is below threshold {:.2}",
                    vmaf.score,
                    threshold
                );

                // Optionally delete low-quality file
                if !options.keep_low_quality {
                    let _ = std::fs::remove_file(output);
                }

                return EncodeResult::QualityBelowThreshold { vmaf, threshold };
            }

            EncodeResult::SuccessWithVmaf(vmaf)
        }
        Err(e) => {
            tracing::warn!(
                "VMAF calculation failed: {}. Reporting success without score.",
                e
            );
            // VMAF failed but encoding succeeded, report success without score
            EncodeResult::Success
        }
    }
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn cleanup_partial_file(path: &str) {
    let _ = std::fs::remove_file(path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_options_default() {
        let opts = EncodeOptions::default();
        assert!(!opts.run_vmaf);
        assert!(opts.vmaf_threshold.is_none());
        assert!(opts.keep_low_quality);
    }

    #[test]
    fn test_encode_options_with_vmaf() {
        let opts = EncodeOptions::default().with_vmaf(Some(90.0));
        assert!(opts.run_vmaf);
        assert_eq!(opts.vmaf_threshold, Some(90.0));
    }
}
