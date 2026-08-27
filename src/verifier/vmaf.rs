use crate::analyzer::HdrType;
use crate::error::AppError;
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tracing::info;

static VMAF_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Escape a value going into a filtergraph argument: `:` separates options, and
/// `\`, `'`, `[`, `]`, `,` and `;` are all meaningful to the parser.
fn escape_filter_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | ':' | '\'' | '[' | ']' | ',' | ';') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Wait for a child process, killing it as soon as the cancel flag is raised.
/// `Ok(None)` means it was cancelled rather than allowed to finish.
fn wait_or_cancel(
    child: &mut std::process::Child,
    cancel_flag: &AtomicBool,
) -> Result<Option<std::process::ExitStatus>, AppError> {
    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
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
    /// Check if quality meets threshold
    pub fn meets_threshold(&self, threshold: f64) -> bool {
        self.score >= threshold
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

/// Calculate VMAF score between original and encoded video. Honours
/// `cancel_flag` as encoding does: the ffmpeg process is killed and its log
/// file cleaned up.
#[allow(clippy::too_many_lines)]
pub fn calculate_vmaf(
    original: &Path,
    encoded: &Path,
    hdr_type: HdrType,
    width: u32,
    cancel_flag: &AtomicBool,
) -> Result<VmafOutcome, AppError> {
    let uid = VMAF_COUNTER.fetch_add(1, Ordering::Relaxed);
    // libvmaf opens `log_path` itself, so the path has to be somewhere nobody
    // else can have pre-planted a symlink under the name.
    let json_output =
        crate::utils::scratch_path(&format!("av1c_vmaf_{}_{}.json", std::process::id(), uid))
            .map_err(AppError::Vmaf)?;

    let (model_suffix, model_name) = if width >= 3840 && hdr_type.is_hdr() {
        (":model='version=vmaf_4k_v0.6.1neg'", "vmaf_4k_v0.6.1neg")
    } else if width >= 3840 {
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
        "[0:v]format=yuv420p10le,setpts=PTS-STARTPTS[ref];\
         [1:v]format=yuv420p10le,setpts=PTS-STARTPTS[dist];\
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
    crate::utils::child::configure(&mut ffmpeg);
    let mut child = ffmpeg.spawn().map_err(|e| {
        let _ = std::fs::remove_file(&stderr_path);
        AppError::CommandExecution(format!("Failed to run ffmpeg for VMAF: {e}"))
    })?;
    let _child = crate::utils::child::ChildGuard::register(child.id());

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
        if stderr.contains("No such filter: 'libvmaf'")
            || stderr.contains("Unknown libvmaf")
            || stderr.contains("Option model not found")
        {
            return Err(AppError::Vmaf(
                "VMAF not available. FFmpeg must be compiled with libvmaf support.".to_string(),
            ));
        }
        return Err(AppError::Vmaf(format!("VMAF calculation failed: {stderr}")));
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
    use super::escape_filter_value;

    #[test]
    fn filter_values_escape_graph_syntax() {
        assert_eq!(escape_filter_value("/tmp/plain.json"), "/tmp/plain.json");
        assert_eq!(
            escape_filter_value("/tmp/od:d[dir]/v.json"),
            "/tmp/od\\:d\\[dir\\]/v.json"
        );
        assert_eq!(escape_filter_value(r"C:\tmp\v.json"), r"C\:\\tmp\\v.json");
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
