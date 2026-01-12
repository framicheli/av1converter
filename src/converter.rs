use crate::analysis::Resolution;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
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
            Resolution::HD1080pDV | Resolution::UHD2160pDV => EncoderProfile::HD1080p,
        }
    }
}

/// Track selection configuration
#[derive(Debug, Clone)]
pub struct TrackSelection {
    pub audio_tracks: Vec<usize>,
    pub subtitle_tracks: Vec<usize>,
}

impl Default for TrackSelection {
    fn default() -> Self {
        Self {
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
        }
    }
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
    ) -> Vec<String> {
        let mut args = vec![
            "-y".to_string(),
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
        args.extend([
            "-c:v".to_string(),
            "libsvtav1".to_string(),
            "-preset".to_string(),
            "4".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p10le".to_string(),
            "-c:a".to_string(),
            "copy".to_string(),
            "-c:s".to_string(),
            "copy".to_string(),
        ]);

        // Profile-specific settings
        match self {
            EncoderProfile::HD1080p => {
                args.extend(["-crf".to_string(), "28".to_string()]);
                args.extend([
                    "-svtav1-params".to_string(),
                    "tune=0:film-grain=0".to_string(),
                ]);
            }
            EncoderProfile::HD1080pHDR => {
                args.extend(["-crf".to_string(), "29".to_string()]);
                args.extend([
                    "-svtav1-params".to_string(),
                    "tune=0:film-grain=1".to_string(),
                ]);
                args.extend([
                    "-color_primaries".to_string(),
                    "bt2020".to_string(),
                    "-color_trc".to_string(),
                    "smpte2084".to_string(),
                    "-colorspace".to_string(),
                    "bt2020nc".to_string(),
                ]);
            }
            EncoderProfile::UHD2160p => {
                args.extend(["-crf".to_string(), "30".to_string()]);
                args.extend([
                    "-svtav1-params".to_string(),
                    "tune=0:film-grain=1".to_string(),
                ]);
            }
            EncoderProfile::UHD2160pHDR => {
                args.extend(["-crf".to_string(), "30".to_string()]);
                args.extend([
                    "-svtav1-params".to_string(),
                    "tune=0:film-grain=1".to_string(),
                ]);
                args.extend([
                    "-color_primaries".to_string(),
                    "bt2020".to_string(),
                    "-color_trc".to_string(),
                    "smpte2084".to_string(),
                    "-colorspace".to_string(),
                    "bt2020nc".to_string(),
                ]);
            }
        }

        // Add progress output for parsing
        args.extend(["-progress".to_string(), "pipe:1".to_string()]);

        args.push(output.to_string());
        args
    }
}

/// Progress callback type for encoding progress updates
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

/// Result of encoding - can be success, error, or cancelled
#[derive(Debug)]
pub enum EncodeResult {
    Success,
    Cancelled,
    Error(String),
}

/// Encode a video file with the given profile and track selection
pub fn encode_video(
    input: &str,
    output: &str,
    resolution: Resolution,
    track_selection: &TrackSelection,
    mut progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
) -> EncodeResult {
    let profile: EncoderProfile = resolution.into();
    let args = profile.build_ffmpeg_args(input, output, track_selection);

    // Get video duration for progress calculation
    let duration = get_video_duration(input).unwrap_or(0.0);

    let mut child = match Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return EncodeResult::Error(format!("Failed to start ffmpeg: {}", e)),
    };

    // Parse progress from stdout
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            // Check for cancellation
            if cancel_flag.load(Ordering::Relaxed) {
                kill_child(&mut child);
                cleanup_partial_file(output);
                return EncodeResult::Cancelled;
            }

            if line.starts_with("out_time_us=") {
                if let Ok(time_us) = line.trim_start_matches("out_time_us=").parse::<f64>() {
                    let time_secs = time_us / 1_000_000.0;
                    if duration > 0.0 {
                        let progress = (time_secs / duration * 100.0).min(100.0) as f32;
                        if let Some(ref mut cb) = progress_callback {
                            cb(progress);
                        }
                    }
                }
            }
        }
    }

    // Final cancellation check before waiting
    if cancel_flag.load(Ordering::Relaxed) {
        kill_child(&mut child);
        cleanup_partial_file(output);
        return EncodeResult::Cancelled;
    }

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return EncodeResult::Error(format!("Failed to wait for ffmpeg: {}", e)),
    };

    if !status.success() {
        cleanup_partial_file(output);
        return EncodeResult::Error(format!("ffmpeg failed with status: {}", status));
    }

    EncodeResult::Success
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

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()
}
