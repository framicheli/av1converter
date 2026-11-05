use crate::analyzer::HdrType;
use crate::analyzer::picture::PictureFilter;
use crate::error::AppError;
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tracing::info;

static VMAF_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Escape a value going into a `-lavfi` filter option. The string is unescaped
/// twice: first by the graph parser (`\ ' [ ] , ;`), then by the option parser
/// (`\ : '`).
fn escape_filter_value(value: &str) -> String {
    let escape = |value: &str, special: &[char]| {
        let mut out = String::with_capacity(value.len());
        for c in value.chars() {
            if special.contains(&c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    escape(
        &escape(value, &['\\', ':', '\'']),
        &['\\', '\'', '[', ']', ',', ';'],
    )
}

/// Wait for a child process, killing it as soon as the cancel flag is raised.
/// `Ok(None)` means it was cancelled rather than allowed to finish.
pub(crate) fn wait_or_cancel(
    child: &mut std::process::Child,
    cancel_flag: &AtomicBool,
) -> Result<Option<std::process::ExitStatus>, AppError> {
    loop {
        if cancel_flag.load(Ordering::Acquire) {
            crate::utils::child::kill_and_wait(child);
            return Ok(None);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                crate::utils::child::ChildGuard::unregister(child.id());
                return Ok(Some(status));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => {
                crate::utils::child::kill_and_wait(child);
                return Err(AppError::Vmaf(format!(
                    "Failed to check VMAF ffmpeg status: {e}"
                )));
            }
        }
    }
}

/// A stretch of the source, in seconds from its start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start_secs: f64,
    pub length_secs: f64,
}

impl Window {
    /// The input options that read only this stretch of the next input.
    pub fn input_args(&self) -> [String; 4] {
        [
            "-ss".to_string(),
            format!("{:.3}", self.start_secs),
            "-t".to_string(),
            format!("{:.3}", self.length_secs),
        ]
    }
}

/// Outcome of a VMAF run that could be interrupted.
pub enum VmafOutcome {
    Scored(VmafResult),
    /// The cancel flag was raised; the ffmpeg process was killed.
    Cancelled,
}

/// VMAF quality result
#[derive(Debug, Clone)]
pub struct VmafResult {
    /// Mean VMAF score (0-100, higher is better)
    pub score: f64,
    /// Minimum frame score
    pub min_score: f64,
    /// Maximum frame score
    pub max_score: f64,
}

impl VmafResult {
    /// Check if quality meets threshold. Both the mean score and the worst
    /// sampled frame must clear it (frames are scored with `n_subsample=10`).
    pub fn meets_threshold(&self, threshold: f64) -> bool {
        self.score >= threshold && self.min_score >= threshold
    }

    /// Get human-readable quality grade
    pub fn quality_grade(&self) -> &'static str {
        let s = self.score;
        if s >= 95.0 {
            "Excellent"
        } else if s >= 90.0 {
            "Very Good"
        } else if s >= 80.0 {
            "Good"
        } else if s >= 70.0 {
            "Fair"
        } else if s >= 60.0 {
            "Poor"
        } else {
            "Bad"
        }
    }
}

impl std::fmt::Display for VmafResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VMAF: {:.2} ({}) [min: {:.2}, max: {:.2}]",
            self.score,
            self.quality_grade(),
            self.min_score,
            self.max_score
        )
    }
}

/// The error a failed libvmaf run reports. [`AppError`]'s own text names the
/// step; the message carries only the reason.
fn vmaf_failure(stderr: String) -> AppError {
    if stderr.contains("No such filter: 'libvmaf'")
        || stderr.contains("Unknown libvmaf")
        || stderr.contains("Option model not found")
    {
        return AppError::Vmaf("FFmpeg must be compiled with libvmaf support".to_string());
    }
    AppError::Vmaf(stderr)
}

/// The `-lavfi` graph that scores input 1 against input 0. The reference
/// passes through `picture` first, the same crop and deinterlace the encode
/// applied. Frames are paired by timestamp, or by position when `picture`
/// drops frames.
fn vmaf_graph(
    log_path: &str,
    n_threads: usize,
    model_suffix: &str,
    picture: &PictureFilter,
) -> String {
    let align = if picture.drops_frames() {
        "settb=1,setpts=N"
    } else {
        "setpts=PTS-STARTPTS"
    };
    let reference = picture
        .chain()
        .map_or_else(String::new, |chain| format!("{chain},"));
    format!(
        "[0:V]{reference}format=yuv420p10le,{align}[ref];\
         [1:V]format=yuv420p10le,{align}[dist];\
         [dist][ref]libvmaf=log_path={}:log_fmt=json:n_threads={n_threads}:n_subsample=10{model_suffix}",
        escape_filter_value(log_path),
    )
}

/// Calculate VMAF score between original and encoded video. With `window`,
/// only that stretch of the original is the reference. Honours `cancel_flag`
/// as encoding does: the ffmpeg process is killed and its log file cleaned
/// up.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn calculate_vmaf(
    original: &Path,
    encoded: &Path,
    window: Option<&Window>,
    hdr_type: HdrType,
    width: u32,
    height: u32,
    picture: &PictureFilter,
    cancel_flag: &AtomicBool,
) -> Result<VmafOutcome, AppError> {
    let uid = VMAF_COUNTER.fetch_add(1, Ordering::Relaxed);
    // libvmaf opens the log path itself; it is a per-process name in the
    // private scratch directory.
    let json_output =
        crate::utils::scratch_path(&format!("av1c_vmaf_{}_{}.json", std::process::id(), uid))
            .map_err(AppError::Vmaf)?;

    // Match encoding-preset tiering: portrait 4K (e.g. 2160×3840) is still UHD.
    let long_side = width.max(height);
    let (model_suffix, model_name) = if long_side >= 3840 && hdr_type.is_hdr() {
        (":model='version=vmaf_4k_v0.6.1neg'", "vmaf_4k_v0.6.1neg")
    } else if long_side >= 3840 {
        (":model='version=vmaf_4k_v0.6.1'", "vmaf_4k_v0.6.1")
    } else if hdr_type.is_hdr() {
        (":model='version=vmaf_v0.6.1neg'", "vmaf_v0.6.1neg")
    } else {
        ("", "vmaf_v0.6.1 (default)")
    };

    // Scale thread count to available cores, capped at 8
    let n_threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));

    // VMAF filter with quick settings (subsample=10 for speed). libvmaf takes
    // the distorted stream as its first input and the reference as its second.
    let filter = vmaf_graph(
        &json_output.to_string_lossy(),
        n_threads,
        model_suffix,
        picture,
    );

    info!(
        "Calculating VMAF: {} vs {} (model: {}, content: {})",
        original.display(),
        encoded.display(),
        model_name,
        hdr_type.display_string()
    );

    // stderr goes to a file, not a pipe; nothing reads it while ffmpeg runs.
    let stderr_path = crate::utils::scratch_path(&format!(
        "av1c_vmaf_stderr_{}_{}.txt",
        std::process::id(),
        uid
    ))
    .map_err(AppError::Vmaf)?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| AppError::Vmaf(format!("Failed to create VMAF log file: {e}")))?;

    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg.args(["-nostdin", "-nostats"]);
    if let Some(window) = window {
        ffmpeg.args(window.input_args());
    }
    ffmpeg
        .args([
            "-i",
            &original.to_string_lossy(),
            "-i",
            &encoded.to_string_lossy(),
            "-lavfi",
            &filter,
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file));
    let (mut child, _child) = crate::utils::child::spawn(&mut ffmpeg).map_err(|e| {
        let _ = std::fs::remove_file(&stderr_path);
        AppError::CommandExecution(format!("Failed to run ffmpeg for VMAF: {e}"))
    })?;

    let waited = wait_or_cancel(&mut child, cancel_flag);
    let status = match waited {
        Ok(Some(status)) => status,
        other => {
            let _ = std::fs::remove_file(&json_output);
            let _ = std::fs::remove_file(&stderr_path);
            return match other {
                Ok(_) => {
                    info!("VMAF check cancelled");
                    Ok(VmafOutcome::Cancelled)
                }
                Err(e) => Err(e),
            };
        }
    };

    let stderr = crate::encoder::ffmpeg::read_file_tail(&stderr_path).unwrap_or_default();
    let _ = std::fs::remove_file(&stderr_path);

    if !status.success() {
        let _ = std::fs::remove_file(&json_output);
        return Err(vmaf_failure(stderr));
    }

    // Read result then remove
    let vmaf_data = std::fs::File::open(&json_output)
        .map(std::io::BufReader::new)
        .map_err(|e| AppError::Vmaf(format!("Failed to read VMAF output: {e}")))
        .and_then(|reader| {
            serde_json::from_reader::<_, VmafJson>(reader)
                .map_err(|e| AppError::Vmaf(format!("Failed to parse VMAF JSON: {e}")))
        });
    let _ = std::fs::remove_file(&json_output);
    let vmaf_data = vmaf_data?;

    let result = VmafResult {
        score: vmaf_data.pooled_metrics.vmaf.mean,
        min_score: vmaf_data.pooled_metrics.vmaf.min,
        max_score: vmaf_data.pooled_metrics.vmaf.max,
    };

    info!("VMAF result: {}", result);

    Ok(VmafOutcome::Scored(result))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::wait_or_cancel;
    use super::{VmafJson, VmafResult, Window, escape_filter_value, vmaf_failure, vmaf_graph};
    use crate::analyzer::picture::{Crop, FieldTreatment, PictureFilter};
    #[cfg(unix)]
    use std::process::Stdio;
    #[cfg(unix)]
    use std::sync::atomic::AtomicBool;

    /// The mean, minimum and maximum come out of libvmaf's own JSON shape.
    #[test]
    fn libvmaf_json_yields_the_pooled_scores() {
        let json = r#"{
            "frames": [{"frameNum": 0, "metrics": {"vmaf": 93.5}}],
            "pooled_metrics": {
                "vmaf": {"min": 91.25, "max": 99.5, "mean": 96.125, "harmonic_mean": 96.0}
            },
            "aggregate_metrics": {}
        }"#;

        let parsed: VmafJson = serde_json::from_str(json).expect("libvmaf output parses");

        assert!((parsed.pooled_metrics.vmaf.mean - 96.125).abs() < f64::EPSILON);
        assert!((parsed.pooled_metrics.vmaf.min - 91.25).abs() < f64::EPSILON);
        assert!((parsed.pooled_metrics.vmaf.max - 99.5).abs() < f64::EPSILON);
    }

    /// Output without the pooled block is refused rather than scored as zero.
    #[test]
    fn vmaf_json_without_pooled_metrics_is_refused() {
        assert!(serde_json::from_str::<VmafJson>(r#"{"frames": []}"#).is_err());
        assert!(serde_json::from_str::<VmafJson>(r#"{"pooled_metrics": {}}"#).is_err());
        assert!(
            serde_json::from_str::<VmafJson>(r#"{"pooled_metrics": {"vmaf": {"mean": 96.0}}}"#)
                .is_err()
        );
    }

    /// A finished child is reported with its status; a cancelled one is killed
    /// and reported as no status at all.
    #[cfg(unix)]
    #[test]
    fn waiting_returns_the_status_or_kills_on_cancel() {
        // Spawned the way the pipeline does: the cancel kills the child's own
        // process group, not the test runner's.
        let (mut quick, _quick) =
            crate::utils::child::spawn(std::process::Command::new("true").stdin(Stdio::null()))
                .expect("true");
        let status = wait_or_cancel(&mut quick, &AtomicBool::new(false))
            .expect("no wait error")
            .expect("a finished child has a status");
        assert!(status.success());

        let (mut slow, _slow) = crate::utils::child::spawn(
            std::process::Command::new("sleep")
                .arg("30")
                .stdin(Stdio::null()),
        )
        .expect("sleep");
        let pid = slow.id();
        assert!(matches!(
            wait_or_cancel(&mut slow, &AtomicBool::new(true)),
            Ok(None)
        ));
        assert!(!crate::utils::child::pid_alive(pid));
    }

    /// The reported reason is not prefixed twice.
    #[test]
    fn a_failed_vmaf_run_names_the_step_once() {
        assert_eq!(
            vmaf_failure("Conversion failed".to_string()).to_string(),
            "VMAF calculation failed: Conversion failed"
        );
        assert_eq!(
            vmaf_failure("No such filter: 'libvmaf'".to_string()).to_string(),
            "VMAF calculation failed: FFmpeg must be compiled with libvmaf support"
        );
    }

    fn scores(score: f64, min_score: f64) -> VmafResult {
        VmafResult {
            score,
            min_score,
            max_score: 100.0,
        }
    }

    #[test]
    fn a_mean_below_the_threshold_fails_even_with_a_passing_minimum() {
        assert!(!scores(89.99, 95.0).meets_threshold(90.0));
    }

    #[test]
    fn a_minimum_below_the_threshold_fails_even_with_a_passing_mean() {
        assert!(!scores(95.0, 89.99).meets_threshold(90.0));
    }

    #[test]
    fn scores_equal_to_the_threshold_pass() {
        assert!(scores(90.0, 90.0).meets_threshold(90.0));
    }

    #[test]
    fn both_scores_below_the_threshold_fail() {
        assert!(!scores(80.0, 70.0).meets_threshold(90.0));
    }

    #[test]
    fn an_unprocessed_reference_is_paired_by_timestamp() {
        assert_eq!(
            vmaf_graph("/tmp/v.json", 4, "", &PictureFilter::default()),
            "[0:V]format=yuv420p10le,setpts=PTS-STARTPTS[ref];\
             [1:V]format=yuv420p10le,setpts=PTS-STARTPTS[dist];\
             [dist][ref]libvmaf=log_path=/tmp/v.json:log_fmt=json:n_threads=4:n_subsample=10"
        );
    }

    #[test]
    fn a_cropped_reference_is_cropped_like_the_encode() {
        let picture = PictureFilter {
            crop: Some(Crop {
                width: 640,
                height: 272,
                x: 0,
                y: 44,
            }),
            fields: FieldTreatment::Deinterlace {
                top_field_first: true,
            },
        };
        assert_eq!(
            vmaf_graph(
                "/tmp/v.json",
                2,
                ":model='version=vmaf_v0.6.1neg'",
                &picture
            ),
            "[0:V]bwdif=mode=send_frame:parity=tff:deint=all,crop=640:272:0:44,\
             format=yuv420p10le,setpts=PTS-STARTPTS[ref];\
             [1:V]format=yuv420p10le,setpts=PTS-STARTPTS[dist];\
             [dist][ref]libvmaf=log_path=/tmp/v.json:log_fmt=json:n_threads=2:\
             n_subsample=10:model='version=vmaf_v0.6.1neg'"
        );
    }

    #[test]
    fn an_inverse_telecined_reference_is_paired_by_frame_position() {
        let picture = PictureFilter {
            crop: None,
            fields: FieldTreatment::InverseTelecine,
        };
        let graph = vmaf_graph("/tmp/v.json", 1, "", &picture);
        assert!(graph.starts_with(
            "[0:V]fieldmatch=order=auto:combmatch=full,\
             bwdif=mode=send_frame:deint=interlaced,decimate,\
             format=yuv420p10le,settb=1,setpts=N[ref];\
             [1:V]format=yuv420p10le,settb=1,setpts=N[dist];"
        ));
        assert!(!graph.contains("STARTPTS"));
    }

    #[test]
    fn a_window_reads_its_stretch_to_the_millisecond() {
        let window = Window {
            start_secs: 125.5,
            length_secs: 8.0,
        };
        assert_eq!(window.input_args(), ["-ss", "125.500", "-t", "8.000"]);
    }

    /// Option-level escapes are escaped again for the graph parser.
    #[test]
    fn filter_values_escape_graph_syntax() {
        assert_eq!(escape_filter_value("/tmp/plain.json"), "/tmp/plain.json");
        assert_eq!(
            escape_filter_value("/tmp/x:y[c],d'e/v.json"),
            r"/tmp/x\\:y\[c\]\,d\\\'e/v.json"
        );
        assert_eq!(
            escape_filter_value(r"C:\Temp\v.json"),
            r"C\\:\\\\Temp\\\\v.json"
        );
    }
}

// JSON deserialization structures

#[derive(Debug, Deserialize)]
struct VmafJson {
    pooled_metrics: PooledMetrics,
}

#[derive(Debug, Deserialize)]
struct PooledMetrics {
    vmaf: MetricStats,
}

#[derive(Debug, Deserialize)]
struct MetricStats {
    mean: f64,
    min: f64,
    max: f64,
}
