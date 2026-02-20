pub mod command_builder;
pub mod ffmpeg;

pub use command_builder::EncodingParams;
pub use ffmpeg::{EncodeResult, ProgressCallback, encode_video};

use crate::analyzer::{HdrType, VideoMetadata};
use crate::config::AppConfig;
use crate::tracks::TrackSelection;
use crate::verifier;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::{info, warn};

/// Full encoding result including VMAF
#[derive(Debug)]
pub enum FullEncodeResult {
    /// Encoding completed successfully
    Success,
    /// Encoding completed with VMAF score
    SuccessWithVmaf(verifier::VmafResult),
    /// Encoding was cancelled
    Cancelled,
    /// Encoding failed
    Error(String),
    /// Quality below threshold
    QualityWarning {
        vmaf: verifier::VmafResult,
        threshold: f64,
    },
}

/// Orchestrate the full encoding pipeline: CRF search -> encode -> verify
#[allow(clippy::too_many_arguments)]
pub fn run_encoding_pipeline(
    input: &str,
    output: &str,
    metadata: &VideoMetadata,
    tracks: TrackSelection,
    config: &AppConfig,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: Arc<AtomicBool>,
) -> FullEncodeResult {
    // Step 1: CRF
    let crf_override = None;

    // Step 2: Build encoding parameters
    let params =
        EncodingParams::from_metadata(input, output, metadata, config, tracks, crf_override);
    let duration = metadata.duration_secs;

    // Step 3: Encode
    let encode_result = encode_video(&params, progress_callback, cancel_flag, duration);

    match encode_result {
        EncodeResult::Success => {
            // Step 4: Verify
            let vmaf_threshold = if config.quality.vmaf_enabled {
                Some(config.quality.vmaf_threshold)
            } else {
                None
            };
            run_vmaf_check(input, output, vmaf_threshold, metadata.hdr_type, metadata.width)
        }
        EncodeResult::Cancelled => FullEncodeResult::Cancelled,
        EncodeResult::Error(e) => FullEncodeResult::Error(e),
    }
}

/// Run VMAF quality check after encoding
fn run_vmaf_check(
    input: &str,
    output: &str,
    threshold: Option<f64>,
    hdr_type: HdrType,
    width: u32,
) -> FullEncodeResult {
    let threshold = match threshold {
        Some(t) => t,
        None => return FullEncodeResult::Success,
    };

    info!("Running VMAF quality check...");

    let input_path = std::path::Path::new(input);
    let output_path = std::path::Path::new(output);

    match verifier::calculate_vmaf(input_path, output_path, hdr_type, width) {
        Ok(vmaf) => {
            info!("VMAF score: {:.2} ({})", vmaf.score, vmaf.quality_grade());

            if !vmaf.meets_threshold(threshold) {
                warn!(
                    "VMAF score {:.2} is below threshold {:.2}",
                    vmaf.score, threshold
                );
                return FullEncodeResult::QualityWarning { vmaf, threshold };
            }

            FullEncodeResult::SuccessWithVmaf(vmaf)
        }
        Err(e) => {
            warn!(
                "VMAF calculation failed: {:?}. Reporting success without score.",
                e
            );
            FullEncodeResult::Success
        }
    }
}
