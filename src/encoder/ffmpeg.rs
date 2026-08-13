use crate::encoder::command_builder::{EncodingParams, build_ffmpeg_args};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Scratch path an in-progress encode is written to, alongside the real output
/// and keeping its extension so `FFmpeg` still infers the container. Renamed
/// onto the destination once the encode finishes.
fn partial_output_path(output: &str, tag: &str) -> String {
    let path = Path::new(output);
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem}.part.{tag}.{ext}"),
        None => format!("{stem}.part.{tag}"),
    };
    parent.join(name).to_string_lossy().into_owned()
}

/// Scratch files left next to `output` by an encode that never finished, as
/// produced by [`partial_output_path`]. The `{pid}_{counter}` tag belongs to a
/// process that is gone, so the match is by shape: both halves must be digits,
/// and a file that merely contains `.part.` is not scratch.
pub fn orphaned_partials(output: &Path) -> Vec<PathBuf> {
    let (Some(parent), Some(stem)) = (output.parent(), output.file_stem().and_then(|s| s.to_str()))
    else {
        return Vec::new();
    };
    let prefix = format!("{stem}.part.");
    let suffix = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| format!(".{ext}"));

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return false;
            };
            let Some(rest) = name.strip_prefix(&prefix) else {
                return false;
            };
            let tag = match suffix.as_deref() {
                Some(suffix) => match rest.strip_suffix(suffix) {
                    Some(tag) => tag,
                    None => return false,
                },
                None => rest,
            };
            matches!(tag.split_once('_'), Some((pid, uid))
                if !pid.is_empty()
                    && !uid.is_empty()
                    && pid.bytes().all(|b| b.is_ascii_digit())
                    && uid.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|entry| entry.path())
        .collect()
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

fn path_occupied(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
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
    // FFmpeg cannot edit a file in place; refuse before spawning.
    if is_same_file(Path::new(&params.input), Path::new(&params.output)) {
        return EncodeResult::Error(
            "Output path is the same as the input file; check the output suffix and container"
                .to_string(),
        );
    }
    if path_occupied(Path::new(&params.output)) {
        return EncodeResult::Error("Output already exists; refusing to overwrite it".to_string());
    }

    // Reserved with `create_new` before FFmpeg sees it, so `-y` cannot erase a
    // real file that happens to sit at the `.part` name.
    let (partial, tag) = loop {
        let uid = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tag = format!("{}_{}", std::process::id(), uid);
        let partial = partial_output_path(&params.output, &tag);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
        {
            Ok(_) => break (partial, tag),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return EncodeResult::Error(format!("Failed to reserve temporary output: {e}"));
            }
        }
    };
    let mut encode_params = params.clone();
    encode_params.output.clone_from(&partial);
    let args = build_ffmpeg_args(&encode_params);

    // Create progress file
    let progress_file = match crate::utils::scratch_path(&format!("av1c_progress_{tag}.txt")) {
        Ok(path) => path,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return EncodeResult::Error(e);
        }
    };
    if File::create(&progress_file).is_err() {
        let _ = std::fs::remove_file(&partial);
        return EncodeResult::Error("Failed to create progress file".to_string());
    }

    // Insert progress args after -nostdin
    let mut args = args;
    args.insert(2, "-progress".to_string());
    args.insert(3, progress_file.to_string_lossy().to_string());

    // Redirect stderr to a temp file to avoid pipe buffer deadlock
    let stderr_path = match crate::utils::scratch_path(&format!("av1c_stderr_{tag}.txt")) {
        Ok(path) => path,
        Err(e) => {
            let _ = std::fs::remove_file(&progress_file);
            let _ = std::fs::remove_file(&partial);
            return EncodeResult::Error(e);
        }
    };
    let stderr_file = match File::create(&stderr_path) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&progress_file);
            let _ = std::fs::remove_file(&partial);
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
            let _ = std::fs::remove_file(&partial);
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
    if matches!(result, EncodeResult::Success) {
        if path_occupied(Path::new(&params.output)) {
            let _ = std::fs::remove_file(&partial);
            return EncodeResult::Error(
                "Output appeared while encoding; refusing to overwrite it".to_string(),
            );
        }
        if let Err(e) = std::fs::rename(&partial, &params.output) {
            let _ = std::fs::remove_file(&partial);
            return EncodeResult::Error(format!("Failed to move the encoded file into place: {e}"));
        }
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
        } else if let Some(content) = read_file_tail(progress_file) {
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
                    let stderr = read_file_tail(stderr_path).unwrap_or_default();

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
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(output);
                return EncodeResult::Error(format!("Failed to check ffmpeg status: {e}"));
            }
        }
    }
}

/// Read the last few progress blocks from `-progress` output. `FFmpeg` appends
/// a block roughly twice a second and never truncates, so only the tail of the
/// file is read.
pub(crate) fn read_file_tail(path: &Path) -> Option<String> {
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
        read_file_tail,
    };
    use std::path::Path;

    /// The scratch file sits next to the real output, keeps its extension, and
    /// is never the destination path itself.
    #[test]
    fn partial_path_is_a_distinct_sibling_with_the_same_extension() {
        let output = "/media/films/movie_av1.mkv";
        let partial = partial_output_path(output, "123_4");

        assert_eq!(partial, "/media/films/movie_av1.part.123_4.mkv");
        assert_ne!(partial, output);
        assert_eq!(Path::new(&partial).parent(), Path::new(output).parent());
        assert_eq!(Path::new(&partial).extension().unwrap(), "mkv");
    }

    #[test]
    fn partial_path_handles_an_extensionless_output() {
        assert_eq!(
            partial_output_path("/tmp/movie", "123_4"),
            "/tmp/movie.part.123_4"
        );
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
    fn file_tail_stays_bounded_and_keeps_the_end() {
        let path =
            std::env::temp_dir().join(format!("av1c_test_file_tail_{}.log", std::process::id()));
        std::fs::write(&path, format!("{}the end", "x".repeat(9000))).unwrap();

        let tail = read_file_tail(&path).unwrap();
        assert_eq!(tail.len(), 8192);
        assert!(tail.ends_with("the end"));
        let _ = std::fs::remove_file(path);
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
