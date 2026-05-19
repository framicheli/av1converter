use crate::encoder::command_builder::{EncodingParams, build_ffmpeg_args};
use std::fs::File;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Progress callback type
pub type ProgressCallback = Box<dyn FnMut(f64) + Send>;

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

/// Encode a video file using `FFmpeg`
pub fn encode_video(
    params: &EncodingParams,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: &AtomicBool,
    duration: f64,
) -> EncodeResult {
    let args = build_ffmpeg_args(params);

    let uid = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tag = format!("{}_{}", std::process::id(), uid);

    // Create progress file
    let progress_file = std::env::temp_dir().join(format!("av1c_progress_{tag}.txt"));
    if File::create(&progress_file).is_err() {
        return EncodeResult::Error("Failed to create progress file".to_string());
    }

    // Insert progress args after -nostdin
    let mut args = args;
    args.insert(2, "-progress".to_string());
    args.insert(3, progress_file.to_string_lossy().to_string());

    // Redirect stderr to a temp file to avoid pipe buffer deadlock
    let stderr_path = std::env::temp_dir().join(format!("av1c_stderr_{tag}.txt"));
    let stderr_file = match File::create(&stderr_path) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&progress_file);
            return EncodeResult::Error(format!("Failed to create stderr file: {e}"));
        }
    };

    // Start FFmpeg
    let mut child = match Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&progress_file);
            let _ = std::fs::remove_file(&stderr_path);
            return EncodeResult::Error(format!("Failed to start ffmpeg: {e}"));
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
        &stderr_path,
    );

    // Cleanup
    let _ = std::fs::remove_file(&progress_file);
    let _ = std::fs::remove_file(&stderr_path);

    result
}

/// Run the encoding loop with progress updates
fn run_encode_loop(
    child: &mut Child,
    progress_file: &Path,
    duration: f64,
    mut progress_callback: Option<ProgressCallback>,
    cancel_flag: &AtomicBool,
    output: &str,
    stderr_path: &Path,
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
                let progress = if duration > 0.0 {
                    (time_secs / duration * 100.0).min(100.0)
                } else {
                    // Duration unknown: advance slowly so UI shows activity (caps at 99%)
                    (time_secs / 7200.0 * 100.0).min(99.0)
                };
                if let Some(ref mut cb) = progress_callback {
                    cb(progress);
                }
            }
        }

        // Check if FFmpeg finished
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let stderr = std::fs::read_to_string(stderr_path).unwrap_or_default();

                    let _ = std::fs::remove_file(output);

                    let error_msg = if stderr.is_empty() {
                        format!("ffmpeg failed with status: {status}")
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
                return EncodeResult::Error(format!("Failed to check ffmpeg status: {e}"));
            }
        }
    }
}
