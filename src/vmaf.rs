//! VMAF (Video Multi-Method Assessment Fusion) Module
//!
//! Provides functionality to calculate VMAF scores between original and
//! encoded videos to validate encoding quality.

use crate::error::AppError;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// VMAF quality assessment result
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
    /// Check if quality meets the specified threshold
    pub fn meets_threshold(&self, threshold: f64) -> bool {
        self.score >= threshold
    }

    /// Get a human-readable quality grade
    pub fn quality_grade(&self) -> &'static str {
        match self.score as u32 {
            95..=100 => "Excellent (Transparent)",
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

/// JSON structure from VMAF output
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

/// Configuration options for VMAF calculation
#[derive(Debug, Clone)]
pub struct VmafOptions {
    /// Number of threads to use (0 = auto)
    pub threads: u32,
    /// Subsample rate (1 = every frame, 5 = every 5th frame)
    pub subsample: u32,
}

impl Default for VmafOptions {
    fn default() -> Self {
        Self {
            threads: 4,
            subsample: 1, // Every frame for full accuracy
        }
    }
}

impl VmafOptions {
    /// Create options optimized for quick estimation (less accurate but faster)
    pub fn quick() -> Self {
        Self {
            threads: 4,
            subsample: 5, // Every 5th frame
        }
    }
}

/// Calculate VMAF score between original and encoded video
pub fn calculate_vmaf(
    original: &Path,
    encoded: &Path,
    options: &VmafOptions,
) -> Result<VmafResult, AppError> {
    let json_output = std::env::temp_dir().join(format!("vmaf_result_{}.json", std::process::id()));

    // Build VMAF filter string using default model bundled with ffmpeg/libvmaf
    let filter = format!(
        "[0:v]format=yuv420p,setpts=PTS-STARTPTS[ref];\
         [1:v]format=yuv420p,setpts=PTS-STARTPTS[dist];\
         [ref][dist]libvmaf=log_path={}:log_fmt=json:n_threads={}:n_subsample={}",
        json_output.to_string_lossy(),
        options.threads,
        options.subsample
    );

    tracing::info!(
        "Calculating VMAF: {} vs {}",
        original.display(),
        encoded.display()
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
        .map_err(|e| AppError::Vmaf {
            message: format!("Failed to run ffmpeg for VMAF: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check if VMAF is not available
        if stderr.contains("No such filter: 'libvmaf'")
            || stderr.contains("Unknown libvmaf")
            || stderr.contains("Option model not found")
        {
            return Err(AppError::Vmaf {
                message: "VMAF is not available. FFmpeg must be compiled with libvmaf support."
                    .to_string(),
            });
        }
        return Err(AppError::Vmaf {
            message: format!("VMAF calculation failed: {}", stderr),
        });
    }

    // Parse JSON result
    let json_content = std::fs::read_to_string(&json_output).map_err(|e| AppError::Vmaf {
        message: format!("Failed to read VMAF output: {}", e),
    })?;

    let _ = std::fs::remove_file(&json_output);

    let vmaf_data: VmafJson = serde_json::from_str(&json_content).map_err(|e| AppError::Vmaf {
        message: format!("Failed to parse VMAF JSON: {}", e),
    })?;

    let result = VmafResult {
        score: vmaf_data.pooled_metrics.vmaf.mean,
        min_score: vmaf_data.pooled_metrics.vmaf.min,
        max_score: vmaf_data.pooled_metrics.vmaf.max,
    };

    tracing::info!("VMAF result: {}", result);

    Ok(result)
}

/// Check if VMAF is available in the current FFmpeg installation
pub fn is_vmaf_available() -> bool {
    let output = Command::new("ffmpeg")
        .args(["-filters"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("libvmaf"));

    output.unwrap_or(false)
}
