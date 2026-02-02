use crate::encoder::command_builder::{EncodingParams, build_ffmpeg_args};
use crate::error::AppError;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::info;

/// Progress callback type
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

/// Encoding result
#[derive(Debug)]
pub enum EncodeResult {
    /// Encoding completed successfully
    Success,
    /// Encoding was cancelled
    Cancelled,
    /// Encoding failed
    Error(String),
}

/// Encode a video file using FFmpeg
pub fn encode_video(
    params: &EncodingParams,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
    duration: f64,
) -> EncodeResult {
    let args = build_ffmpeg_args(params);

    // Create progress file
    let progress_file =
        std::env::temp_dir().join(format!("ffmpeg_progress_{}", std::process::id()));
    if std::fs::File::create(&progress_file).is_err() {
        return EncodeResult::Error("Failed to create progress file".to_string());
    }

    // Insert progress args after -nostdin
    let mut args = args;
    args.insert(2, "-progress".to_string());
    args.insert(3, progress_file.to_string_lossy().to_string());

    info!(
        "Encoding: {} -> {} with {}",
        params.input, params.output, params.encoder
    );

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
        &params.output,
    );

    // Cleanup
    let _ = std::fs::remove_file(&progress_file);

    result
}

/// Get video duration in seconds via ffprobe
pub fn get_duration(input: &str) -> Option<f64> {
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

/// Get encoded file's frame rate as num/den
pub fn get_frame_rate(path: &str) -> Result<(u32, u32), AppError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=r_frame_rate",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path,
        ])
        .output()
        .map_err(|e| AppError::Validation(format!("Failed to run ffprobe: {}", e)))?;

    let rate_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = rate_str.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<u32>().unwrap_or(0);
        let den = parts[1].parse::<u32>().unwrap_or(1);
        Ok((num, den))
    } else {
        Err(AppError::Validation(format!(
            "Unexpected frame rate format: {}",
            rate_str
        )))
    }
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
