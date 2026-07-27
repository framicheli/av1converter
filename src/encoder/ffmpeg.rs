use crate::encoder::command_builder::{EncodingParams, build_ffmpeg_args};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Scratch path an in-progress encode is written to, alongside the real output.
///
/// The final extension is preserved so `FFmpeg` still infers the container from
/// it. Encoding here and renaming on success means the destination file is only
/// ever touched by an encode that actually finished.
fn partial_output_path(output: &str) -> String {
    let path = Path::new(output);
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem}.part.{ext}"),
        None => format!("{stem}.part"),
    };
    parent.join(name).to_string_lossy().into_owned()
}

/// Whether both paths designate the same file: literally equal, or resolving to
/// the same canonical target when both already exist.
fn is_same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

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
    // FFmpeg cannot edit a file in place, and a run that started anyway would
    // end up renaming its own output over the source. Refuse before spawning.
    if is_same_file(Path::new(&params.input), Path::new(&params.output)) {
        return EncodeResult::Error(
            "Output path is the same as the input file; check the output suffix and container"
                .to_string(),
        );
    }

    // FFmpeg's -y truncates the output the moment it opens it, so encoding
    // straight to the destination destroys whatever is already there even when
    // the encode then fails. Write to a sibling scratch file and move it into
    // place only once FFmpeg has exited successfully.
    let partial = partial_output_path(&params.output);
    let mut encode_params = params.clone();
    encode_params.output.clone_from(&partial);
    let args = build_ffmpeg_args(&encode_params);

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
        &partial,
        &stderr_path,
        params.remux_only.then(|| params.input.clone()).as_ref(),
    );

    // Cleanup
    let _ = std::fs::remove_file(&progress_file);
    let _ = std::fs::remove_file(&stderr_path);

    // The scratch file becomes the output only now, when the encode is known to
    // have succeeded. Failure and cancellation already removed it.
    if matches!(result, EncodeResult::Success)
        && let Err(e) = std::fs::rename(&partial, &params.output)
    {
        let _ = std::fs::remove_file(&partial);
        return EncodeResult::Error(format!("Failed to move the encoded file into place: {e}"));
    }

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
        } else if let Some(content) = read_progress_tail(progress_file) {
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

/// Read the last few progress blocks from `-progress` output.
///
/// `FFmpeg` appends a block roughly twice a second and never truncates, so a
/// feature-length encode leaves megabytes behind. Only the most recent block
/// matters, and re-reading and re-parsing the whole file four times a second
/// would cost more as the encode goes on.
fn read_progress_tail(path: &Path) -> Option<String> {
    /// Comfortably more than one block, so a full block is always in view.
    const WINDOW: usize = 8192;

    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(WINDOW as u64)))
        .ok()?;

    let mut buf = Vec::with_capacity(WINDOW);
    file.read_to_end(&mut buf).ok()?;
    // The window can start mid-line; parsing is per-line, so a partial first
    // line is simply ignored by the callers.
    Some(String::from_utf8_lossy(&buf).into_owned())
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
    use super::{
        is_same_file, latest_progress_frame, latest_progress_time_secs, partial_output_path,
    };
    use std::path::Path;

    /// The scratch file sits next to the real output and keeps its extension,
    /// so `FFmpeg` still picks the right muxer and the rename stays on one
    /// filesystem. Crucially it is never the destination path itself.
    #[test]
    fn partial_path_is_a_distinct_sibling_with_the_same_extension() {
        let output = "/media/films/movie_av1.mkv";
        let partial = partial_output_path(output);

        assert_eq!(partial, "/media/films/movie_av1.part.mkv");
        assert_ne!(partial, output);
        assert_eq!(Path::new(&partial).parent(), Path::new(output).parent());
        assert_eq!(Path::new(&partial).extension().unwrap(), "mkv");
    }

    #[test]
    fn partial_path_handles_an_extensionless_output() {
        assert_eq!(partial_output_path("/tmp/movie"), "/tmp/movie.part");
    }

    #[test]
    fn same_file_detects_identical_paths() {
        assert!(is_same_file(
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a.mkv")
        ));
        assert!(!is_same_file(
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a_av1.mkv")
        ));
    }

    /// Two spellings of one existing file resolve to the same target.
    #[test]
    fn same_file_resolves_equivalent_spellings() {
        let dir = std::env::temp_dir();
        let path = dir.join("av1c_test_same_file.mkv");
        std::fs::write(&path, b"x").unwrap();
        let indirect = dir.join(".").join("av1c_test_same_file.mkv");

        assert!(is_same_file(&path, &indirect));
        let _ = std::fs::remove_file(&path);
    }

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
