use crate::analyzer::HdrType;
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
fn wait_or_cancel(
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
    /// sampled frame must clear the threshold so a few catastrophic scenes
    /// cannot hide behind a passing average (frames are scored with
    /// `n_subsample=10`).
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
/// step, so the message carries only the reason.
fn vmaf_failure(stderr: String) -> AppError {
    if stderr.contains("No such filter: 'libvmaf'")
        || stderr.contains("Unknown libvmaf")
        || stderr.contains("Option model not found")
    {
        return AppError::Vmaf("FFmpeg must be compiled with libvmaf support".to_string());
    }
    AppError::Vmaf(stderr)
}

/// Calculate VMAF score between original and encoded video. Honours
/// `cancel_flag` as encoding does: the ffmpeg process is killed and its log
/// file cleaned up.
#[allow(clippy::too_many_lines)]
pub fn calculate_vmaf(
    original: &Path,
    encoded: &Path,
    hdr_type: HdrType,
    width: u32,
    height: u32,
    cancel_flag: &AtomicBool,
) -> Result<VmafOutcome, AppError> {
    let uid = VMAF_COUNTER.fetch_add(1, Ordering::Relaxed);
    // libvmaf opens `log_path` itself, so the path has to be somewhere nobody
    // else can have pre-planted a symlink under the name.
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
    let filter = format!(
        "[0:V]format=yuv420p10le,setpts=PTS-STARTPTS[ref];\
         [1:V]format=yuv420p10le,setpts=PTS-STARTPTS[dist];\
         [dist][ref]libvmaf=log_path={}:log_fmt=json:n_threads={}:n_subsample=10{}",
        escape_filter_value(&json_output.to_string_lossy()),
        n_threads,
        model_suffix
    );

    info!(
        "Calculating VMAF: {} vs {} (model: {}, content: {})",
        original.display(),
        encoded.display(),
        model_name,
        hdr_type.display_string()
    );

    // stderr goes to a file, not a pipe: nothing here reads it while ffmpeg
    // runs, and a full pipe buffer blocks the child.
    let stderr_path = crate::utils::scratch_path(&format!(
        "av1c_vmaf_stderr_{}_{}.txt",
        std::process::id(),
        uid
    ))
    .map_err(AppError::Vmaf)?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| AppError::Vmaf(format!("Failed to create VMAF log file: {e}")))?;

    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg
        .args([
            "-nostdin",
            "-nostats",
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
    use super::{VmafResult, escape_filter_value, vmaf_failure};

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
