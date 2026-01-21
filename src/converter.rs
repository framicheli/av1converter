use crate::analysis::Resolution;
use crate::encoder::{AV1Encoder, QualityProfile, get_hdr_params, get_quality_params};
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

impl EncoderProfile {
    pub fn build_ffmpeg_args(
        &self,
        input: &str,
        output: &str,
        track_selection: &TrackSelection,
        encoder: AV1Encoder,
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
        args.extend(get_quality_params(encoder, quality_profile));

        // Add HDR parameters if needed
        args.extend(get_hdr_params(quality_profile));

        args.push(output.to_string());
        args
    }
}

/// Progress callback type for encoding progress updates
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

/// Result of encoding
#[derive(Debug)]
pub enum EncodeResult {
    Success,
    Cancelled,
    Error(String),
}

/// Encode video
pub fn encode_video(
    input: &str,
    output: &str,
    resolution: Resolution,
    track_selection: &TrackSelection,
    encoder: AV1Encoder,
    mut progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
) -> EncodeResult {
    let profile: EncoderProfile = resolution.into();
    let mut args = profile.build_ffmpeg_args(input, output, track_selection, encoder);

    // Get video duration for progress calculation
    let duration = get_video_duration(input).unwrap_or(0.0);

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

    let mut child = match Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&progress_file);
            return EncodeResult::Error(format!("Failed to start ffmpeg: {}", e));
        }
    };

    // Main loop: poll progress file and check for completion
    loop {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            kill_child(&mut child);
            cleanup_partial_file(output);
            let _ = std::fs::remove_file(&progress_file);
            return EncodeResult::Cancelled;
        }

        // Read progress file and find the latest out_time_us value
        if let Ok(content) = std::fs::read_to_string(&progress_file) {
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
                let _ = std::fs::remove_file(&progress_file);
                if !status.success() {
                    cleanup_partial_file(output);
                    return EncodeResult::Error(format!("ffmpeg failed with status: {}", status));
                }
                return EncodeResult::Success;
            }
            Ok(None) => {
                // Still running
                thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&progress_file);
                return EncodeResult::Error(format!("Failed to check ffmpeg status: {}", e));
            }
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
