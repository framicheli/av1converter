use crate::analyzer::HdrType;
use crate::error::AppError;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use tracing::info;

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
        match self.score as u32 {
            95..=100 => "Excellent",
            90..=94 => "Very Good",
            80..=89 => "Good",
            70..=79 => "Fair",
            60..=69 => "Poor",
            _ => "Bad",
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

/// Calculate VMAF score between original and encoded video
pub fn calculate_vmaf(
    original: &Path,
    encoded: &Path,
    hdr_type: HdrType,
    width: u32,
) -> Result<VmafResult, AppError> {
    let json_output = std::env::temp_dir().join(format!("vmaf_result_{}.json", std::process::id()));

    let (model_suffix, model_name) = if width >= 3840 {
        (":model='version=vmaf_4k_v0.6.1'", "vmaf_4k_v0.6.1")
    } else if hdr_type.is_hdr() {
        (":model='version=vmaf_v0.6.1neg'", "vmaf_v0.6.1neg")
    } else {
        ("", "vmaf_v0.6.1 (default)")
    };

    // VMAF filter with quick settings (subsample=10 for speed)
    let filter = format!(
        "[0:v]format=yuv420p10le,setpts=PTS-STARTPTS[ref];\
         [1:v]format=yuv420p10le,setpts=PTS-STARTPTS[dist];\
         [ref][dist]libvmaf=log_path={}:log_fmt=json:n_threads=4:n_subsample=10{}",
        json_output.to_string_lossy(),
        model_suffix
    );

    info!(
        "Calculating VMAF: {} vs {} (model: {}, content: {})",
        original.display(),
        encoded.display(),
        model_name,
        hdr_type.display_string()
    );

    let output = Command::new("ffmpeg")
        .args([
            "-i",
            original.to_str().unwrap_or(""),
            "-i",
            encoded.to_str().unwrap_or(""),
            "-lavfi",
            &filter,
            "-f",
            "null",
            "-",
        ])
        .output()
        .map_err(|e| AppError::CommandExecution(format!("Failed to run ffmpeg for VMAF: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("No such filter: 'libvmaf'")
            || stderr.contains("Unknown libvmaf")
            || stderr.contains("Option model not found")
        {
            return Err(AppError::Vmaf(
                "VMAF not available. FFmpeg must be compiled with libvmaf support.".to_string(),
            ));
        }
        return Err(AppError::Vmaf(format!(
            "VMAF calculation failed: {}",
            stderr
        )));
    }

    // Parse JSON result
    let json_content = std::fs::read_to_string(&json_output)
        .map_err(|e| AppError::Vmaf(format!("Failed to read VMAF output: {}", e)))?;

    let _ = std::fs::remove_file(&json_output);

    let vmaf_data: VmafJson = serde_json::from_str(&json_content)
        .map_err(|e| AppError::Vmaf(format!("Failed to parse VMAF JSON: {}", e)))?;

    let result = VmafResult {
        score: vmaf_data.pooled_metrics.vmaf.mean,
        min_score: vmaf_data.pooled_metrics.vmaf.min,
        max_score: vmaf_data.pooled_metrics.vmaf.max,
    };

    info!("VMAF result: {}", result);

    Ok(result)
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
