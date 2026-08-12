pub mod command_builder;
#[cfg(test)]
mod end_to_end;
pub mod ffmpeg;

pub use command_builder::EncodingParams;
pub use ffmpeg::{EncodeResult, ProgressCallback, encode_video, orphaned_partials};

use crate::analyzer::{DvMode, HdrType, VideoMetadata};
use crate::config::AppConfig;
use crate::queue::SourceIdentity;
use crate::tracks::OutputTracks;
use crate::verifier;
use std::fs::File;
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

/// Orchestrate the full encoding pipeline: encode -> verify
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn run_encoding_pipeline(
    input: &str,
    output: &str,
    expected_source: &SourceIdentity,
    metadata: &VideoMetadata,
    tracks: OutputTracks,
    dv_mode: DvMode,
    remux_only: bool,
    subtitle_codecs: Vec<&'static str>,
    config: &AppConfig,
    progress_callback: Option<ProgressCallback>,
    cancel_flag: &AtomicBool,
    on_before_vmaf: Option<Box<dyn FnOnce() + Send>>,
) -> FullEncodeResult {
    if !expected_source.matches_path(input) {
        return FullEncodeResult::Error(
            "Source changed since it was analyzed; remove and add it again".to_string(),
        );
    }
    // Keep the original file open and fingerprinted for the entire pipeline.
    // Auto-delete must never remove a replacement that appeared at the path
    // while a long encode or VMAF run was in progress.
    let _source_guard = if config.quality.delete_source_on_success {
        match File::open(input) {
            Ok(file)
                if file
                    .metadata()
                    .ok()
                    .map(|metadata| SourceIdentity::from_metadata(&metadata))
                    .as_ref()
                    == Some(expected_source) =>
            {
                Some(file)
            }
            Ok(_) => {
                return FullEncodeResult::Error(
                    "Source changed while preparing the job; remove and add it again".to_string(),
                );
            }
            Err(e) => {
                return FullEncodeResult::Error(format!(
                    "Could not safely hold the source file open: {e}"
                ));
            }
        }
    } else {
        None
    };

    // Encoding parameters
    let params = EncodingParams::from_metadata(
        input,
        output,
        metadata,
        config,
        tracks,
        dv_mode,
        remux_only,
        subtitle_codecs,
    );
    let duration = metadata.duration_secs;

    // Total frame count for frame-based progress fallback (some sources, e.g.
    // Dolby Vision, make FFmpeg report out_time=N/A but still emit frame counts).
    let total_frames = if metadata.frame_rate_den > 0 {
        duration * f64::from(metadata.frame_rate_num) / f64::from(metadata.frame_rate_den)
    } else {
        0.0
    };

    // Encode
    let encode_result = encode_video(
        &params,
        progress_callback,
        cancel_flag,
        duration,
        total_frames,
    );

    match encode_result {
        EncodeResult::Success => {
            // VMAF and source deletion refer to the output by path. Remember
            // the file the encoder actually placed there so a replacement
            // cannot be verified and then left behind after deleting source.
            let output_identity = config
                .quality
                .delete_source_on_success
                .then(|| SourceIdentity::from_path(output).ok())
                .flatten();
            // DV profile 5 → HDR10 is a tone-mapping pass: output pixels are
            // intentionally different from the source, so VMAF is meaningless.
            let tone_mapped = metadata.hdr_type == HdrType::DolbyVision
                && metadata.dv_profile == Some(5)
                && dv_mode == DvMode::ToHdr10;
            if tone_mapped && config.quality.vmaf_enabled {
                info!("Skipping VMAF: DV profile 5 tone-mapped output is not comparable");
            }

            // Notify the UI for VMAF verification phase
            let vmaf_threshold = if config.quality.vmaf_enabled && !remux_only && !tone_mapped {
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
                cancel_flag,
            );

            // VMAF compares video and nothing else, so a passing score says
            // nothing about audio that was re-encoded to a lossy codec. The
            // original is the only remaining copy of a lossless TrueHD or
            // DTS-HD track, and no automated check here can vouch for what
            // replaced it.
            let audio_transcoded = params.tracks.transcodes_audio();
            if audio_transcoded && config.quality.delete_source_on_success {
                info!(
                    "Keeping source file {input}: audio was transcoded and VMAF does not verify it"
                );
            }

            // The source is only ever deleted against a VMAF score that met the
            // threshold. A plain `Success` means no comparison ran at all (VMAF
            // disabled, a remux, or a tone-mapped DV profile 5 output), which is
            // no evidence that the encode is good enough to discard the original.
            if config.quality.delete_source_on_success && !audio_transcoded {
                if let FullEncodeResult::SuccessWithVmaf {
                    ref mut source_deleted,
                    ..
                } = result
                {
                    if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        warn!("Keeping source file {input}: cancellation was requested");
                    } else if !expected_source.matches_path(input) {
                        warn!("Keeping source file {input}: it changed while the job was running");
                    } else if !output_identity
                        .as_ref()
                        .is_some_and(|identity| identity.matches_path(output))
                    {
                        warn!(
                            "Keeping source file {input}: the encoded output changed before deletion"
                        );
                    } else {
                        match std::fs::remove_file(input) {
                            Ok(()) => {
                                info!("Deleted source file: {input}");
                                *source_deleted = true;
                            }
                            Err(e) => warn!("Failed to delete source file {input}: {e}"),
                        }
                    }
                } else if matches!(result, FullEncodeResult::Success) {
                    info!("Keeping source file {input}: no VMAF verification ran for this job");
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
    cancel_flag: &AtomicBool,
) -> FullEncodeResult {
    let Some(threshold) = threshold else {
        return FullEncodeResult::Success;
    };

    info!("Running VMAF quality check...");

    let input_path = std::path::Path::new(input);
    let output_path = std::path::Path::new(output);

    match verifier::calculate_vmaf(input_path, output_path, hdr_type, width, cancel_flag) {
        // The encode itself finished, so the output stays; only the quality
        // check was interrupted.
        Ok(verifier::VmafOutcome::Cancelled) => FullEncodeResult::Cancelled,
        Ok(verifier::VmafOutcome::Scored(vmaf)) => {
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

#[cfg(test)]
mod tests {
    use super::SourceIdentity;

    #[test]
    fn source_identity_rejects_a_replacement_file() {
        let dir = std::env::temp_dir().join(format!("av1c_source_id_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("movie.mkv");
        std::fs::write(&path, b"old").unwrap();
        let original = std::fs::File::open(&path).unwrap();
        let identity = SourceIdentity::from_metadata(&original.metadata().unwrap());
        assert!(identity.matches_path(path.to_str().unwrap()));

        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"new").unwrap();
            assert!(!identity.matches_path(path.to_str().unwrap()));
        }

        let _ = std::fs::remove_dir_all(dir);
    }
}
