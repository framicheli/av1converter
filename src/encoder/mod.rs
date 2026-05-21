pub mod command_builder;
pub mod ffmpeg;

pub use command_builder::EncodingParams;
pub use ffmpeg::{EncodeResult, ProgressCallback, encode_video};

use crate::analyzer::{HdrType, VideoMetadata};
use crate::config::AppConfig;
use crate::tracks::TrackSelection;
use crate::verifier;
use std::sync::atomic::AtomicBool;
use tracing::{info, warn};

/// Full encoding result including VMAF
#[derive(Debug)]
pub enum FullEncodeResult {
    /// Encoding completed successfully (VMAF disabled)
    Success,
    /// Encoding completed with VMAF score meeting the threshold
    SuccessWithVmaf {
        vmaf: verifier::VmafResult,
        source_deleted: bool,
    },
    /// Encoding succeeded but VMAF check could not be run (e.g. libvmaf missing)
    VmafFailed { message: String },
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

impl FullEncodeResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::SuccessWithVmaf { .. })
    }
}

/// Orchestrate the full encoding pipeline: encode -> verify
#[allow(clippy::too_many_arguments)]
pub fn run_encoding_pipeline(
    input: &str,
    output: &str,
    metadata: &VideoMetadata,
    tracks: TrackSelection,
    remux_only: bool,
    config: &AppConfig,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: &AtomicBool,
    on_before_vmaf: Option<Box<dyn FnOnce() + Send>>,
) -> FullEncodeResult {
    // Encoding parameters
    let params = EncodingParams::from_metadata(input, output, metadata, config, tracks, remux_only);
    let duration = metadata.duration_secs;

    // Encode
    let encode_result = encode_video(&params, progress_callback, cancel_flag, duration);

    match encode_result {
        EncodeResult::Success => {
            // Notify the UI for VMAF verification phase
            let vmaf_threshold = if config.quality.vmaf_enabled && !remux_only {
                if let Some(cb) = on_before_vmaf {
                    cb();
                }
                Some(config.quality.vmaf_threshold)
            } else {
                None
            };
            let mut result = run_vmaf_check(
                input,
                output,
                vmaf_threshold,
                metadata.hdr_type,
                metadata.width,
            );

            // Optionally delete source after any successful encode
            if config.quality.delete_source_on_success && result.is_success() {
                match std::fs::remove_file(input) {
                    Ok(()) => {
                        info!("Deleted source file: {input}");
                        if let FullEncodeResult::SuccessWithVmaf {
                            ref mut source_deleted,
                            ..
                        } = result
                        {
                            *source_deleted = true;
                        }
                    }
                    Err(e) => warn!("Failed to delete source file {input}: {e}"),
                }
            }

            result
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
    let Some(threshold) = threshold else {
        return FullEncodeResult::Success;
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

            FullEncodeResult::SuccessWithVmaf {
                vmaf,
                source_deleted: false,
            }
        }
        Err(e) => {
            warn!("VMAF calculation failed: {:?}", e);
            FullEncodeResult::VmafFailed {
                message: e.to_string(),
            }
        }
    }
}
