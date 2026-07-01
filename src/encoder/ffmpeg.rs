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
    total_frames: f64,
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
        total_frames,
        progress_callback,
        cancel_flag,
        &params.output,
        &stderr_path,
        params.remux_only.then(|| params.input.clone()).as_ref(),
    );

    // Cleanup
    let _ = std::fs::remove_file(&progress_file);
    let _ = std::fs::remove_file(&stderr_path);

    result
}

/// Run the encoding loop with progress updates
///
#[allow(clippy::too_many_arguments)]
fn run_encode_loop(
    child: &mut Child,
    progress_file: &Path,
    duration: f64,
    total_frames: f64,
    mut progress_callback: Option<ProgressCallback>,
    cancel_flag: &AtomicBool,
    output: &str,
    stderr_path: &Path,
    remux_input: Option<&String>,
) -> EncodeResult {
    // For remux jobs, estimate the final output size from the source file size.
    let remux_input_size = remux_input
        .and_then(|input| std::fs::metadata(input).ok())
        .map(|m| m.len())
        .filter(|&len| len > 0);

    loop {
        // Check cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(output);
            return EncodeResult::Cancelled;
        }

        if let Some(input_size) = remux_input_size {
            // Remux: derive progress from output-size growth vs. source size.
            if let Ok(out_meta) = std::fs::metadata(output) {
                #[allow(clippy::cast_precision_loss)]
                let progress = (out_meta.len() as f64 / input_size as f64 * 100.0).min(99.0);
                if let Some(ref mut cb) = progress_callback {
                    cb(progress);
                }
            }
        } else if let Ok(content) = std::fs::read_to_string(progress_file) {
            // Encode: derive progress from the processed timestamp vs. duration.
            // FFmpeg versions differ in which out_time field they emit.
            let progress = if let Some(time_secs) = latest_progress_time_secs(&content) {
                Some(if duration > 0.0 {
                    (time_secs / duration * 100.0).min(100.0)
                } else {
                    // Duration unknown: advance slowly so UI shows activity (caps at 99%)
                    (time_secs / 7200.0 * 100.0).min(99.0)
                })
            } else if total_frames > 0.0 {
                // Some sources (e.g. Dolby Vision) make FFmpeg report out_time=N/A.
                // Fall back to the processed frame count vs. the total frame count.
                latest_progress_frame(&content)
                    .map(|frame| (frame / total_frames * 100.0).min(99.0))
            } else {
                None
            };

            if let Some(progress) = progress
                && let Some(ref mut cb) = progress_callback
            {
                cb(progress);
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
                // Remux copies finish quickly, so poll more often to collect
                // enough progress samples for a meaningful ETA.
                let poll_ms = if remux_input_size.is_some() { 100 } else { 250 };
                thread::sleep(Duration::from_millis(poll_ms));
            }
            Err(e) => {
                return EncodeResult::Error(format!("Failed to check ffmpeg status: {e}"));
            }
        }
    }
}

fn latest_progress_time_secs(content: &str) -> Option<f64> {
    content
        .lines()
        .filter_map(parse_progress_time_secs)
        .next_back()
}

fn latest_progress_frame(content: &str) -> Option<f64> {
    content
        .lines()
        .filter_map(|line| {
            line.strip_prefix("frame=")?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|frame| *frame > 0.0)
        })
        .next_back()
}

fn parse_progress_time_secs(line: &str) -> Option<f64> {
    if let Some(value) = line
        .strip_prefix("out_time_us=")
        .or_else(|| line.strip_prefix("out_time_ms="))
    {
        return value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|time_us| *time_us > 0.0)
            .map(|time_us| time_us / 1_000_000.0);
    }

    parse_out_time_secs(line)
}

fn parse_out_time_secs(line: &str) -> Option<f64> {
    let value = line.strip_prefix("out_time=")?.trim();
    let mut parts = value.split(':');

    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;

    if parts.next().is_some() {
        return None;
    }

    let total = hours * 3600.0 + minutes * 60.0 + seconds;
    (total > 0.0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::{latest_progress_frame, latest_progress_time_secs};

    #[test]
    fn parses_latest_frame_count() {
        let progress = "frame=10\nout_time=N/A\nframe=42\nout_time=N/A\n";

        assert_eq!(latest_progress_frame(progress), Some(42.0));
    }

    #[test]
    fn ignores_zero_frame_count() {
        let progress = "frame=0\nout_time=N/A\n";

        assert_eq!(latest_progress_frame(progress), None);
    }

    #[test]
    fn parses_latest_out_time_us() {
        let progress = "out_time_us=1000000\nprogress=continue\nout_time_us=2500000\n";

        assert_eq!(latest_progress_time_secs(progress), Some(2.5));
    }

    #[test]
    fn parses_legacy_out_time_ms_as_microseconds() {
        let progress = "out_time_ms=1500000\nprogress=continue\n";

        assert_eq!(latest_progress_time_secs(progress), Some(1.5));
    }

    #[test]
    fn parses_textual_out_time() {
        let progress = "out_time=N/A\nout_time=01:02:03.500000\n";

        assert_eq!(latest_progress_time_secs(progress), Some(3723.5));
    }
}
