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
    subtitle_codecs: Vec<Option<&'static str>>,
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
    // The original stays open and fingerprinted for the whole pipeline, so a
    // file that replaces it at the same path is not taken for the original.
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
            // VMAF and source deletion refer to the output by path, so the
            // file the encoder placed there is remembered and re-checked.
            let output_identity = config
                .quality
                .delete_source_on_success
                .then(|| SourceIdentity::from_path(output).ok())
                .flatten();
            let tone_mapped = skips_vmaf(&params);
            if tone_mapped && config.quality.vmaf_enabled {
                info!("Skipping VMAF: DV profile 5 tone-mapped output is not comparable");
            }

            // A cancel that arrived after ffmpeg exited but before verification
            // should not leave a finished output that blocks the next run.
            if cancel_flag.load(std::sync::atomic::Ordering::Acquire) {
                let _ = std::fs::remove_file(output);
                return FullEncodeResult::Cancelled;
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
                metadata.height,
                cancel_flag,
            );

            // The source is only ever deleted against a VMAF score that met the
            // threshold. A plain `Success` means no comparison ran at all (VMAF
            // disabled, a remux, or a tone-mapped DV profile 5 output).
            if config.quality.delete_source_on_success {
                if let FullEncodeResult::SuccessWithVmaf {
                    ref mut source_deleted,
                    ..
                } = result
                {
                    if cancel_flag.load(std::sync::atomic::Ordering::Acquire) {
                        warn!("Keeping source file {input}: cancellation was requested");
                    } else if let Some(reason) = keep_source_reason(&params, cancel_flag) {
                        info!("Keeping source file {input}: {reason}");
                    } else if !expected_source.matches_path(input) {
                        warn!("Keeping source file {input}: it changed while the job was running");
                    } else if !output_identity
                        .as_ref()
                        .is_some_and(|identity| identity.matches_path(output))
                    {
                        warn!(
                            "Keeping source file {input}: the encoded output changed before deletion"
                        );
                    } else if let Err(e) = flush_to_disk(output) {
                        warn!(
                            "Keeping source file {input}: the encoded output could not be flushed to disk: {e}"
                        );
                    } else if std::fs::symlink_metadata(input)
                        .is_ok_and(|m| m.file_type().is_symlink())
                    {
                        warn!("Keeping source file {input}: it is a symbolic link");
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

/// Flush `path`'s contents, and on Unix its directory entry, to disk.
fn flush_to_disk(path: &str) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .sync_all()?;
    #[cfg(unix)]
    if let Some(dir) = std::path::Path::new(path).parent() {
        let dir = if dir.as_os_str().is_empty() {
            std::path::Path::new(".")
        } else {
            dir
        };
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// What the output loses that VMAF does not measure, which keeps the source
/// whatever the score: transcoded audio, subtitles converted or left out, and
/// cover art or attachments the output container does not carry.
fn keep_source_reason(params: &EncodingParams, cancel: &AtomicBool) -> Option<&'static str> {
    if params.tracks.transcodes_audio() {
        return Some("audio was transcoded and VMAF does not verify it");
    }
    if params
        .subtitle_codecs
        .iter()
        .any(|codec| *codec != Some("copy"))
    {
        return Some("a selected subtitle track was converted or left out");
    }
    match crate::analyzer::ffprobe::probe_attachments(&params.input, cancel) {
        Some((0, 0)) => None,
        Some((0, _)) if command_builder::keeps_attachments(&params.output) => None,
        Some(_) => Some("its cover art or attachments are not carried into the output"),
        None => Some("its attachments could not be checked"),
    }
}

/// Run VMAF quality check after encoding
fn run_vmaf_check(
    input: &str,
    output: &str,
    threshold: Option<f64>,
    hdr_type: HdrType,
    width: u32,
    height: u32,
    cancel_flag: &AtomicBool,
) -> FullEncodeResult {
    let Some(threshold) = threshold else {
        return FullEncodeResult::Success;
    };

    info!("Running VMAF quality check...");

    let input_path = std::path::Path::new(input);
    let output_path = std::path::Path::new(output);

    match verifier::calculate_vmaf(
        input_path,
        output_path,
        hdr_type,
        width,
        height,
        cancel_flag,
    ) {
        // Cancel during verification: drop the finished output so a retry is
        // not blocked by "output already exists", and report Cancelled.
        Ok(verifier::VmafOutcome::Cancelled) => {
            let _ = std::fs::remove_file(output);
            FullEncodeResult::Cancelled
        }
        Ok(verifier::VmafOutcome::Scored(vmaf)) => {
            info!("VMAF score: {:.2} ({})", vmaf.score, vmaf.quality_grade());

            if !vmaf.meets_threshold(threshold) {
                if vmaf.min_score < threshold && vmaf.score >= threshold {
                    warn!(
                        "VMAF min frame {:.2} is below threshold {:.2} (mean {:.2})",
                        vmaf.min_score, threshold, vmaf.score
                    );
                } else if vmaf.score < threshold && vmaf.min_score >= threshold {
                    warn!(
                        "VMAF mean {:.2} is below threshold {:.2} (min {:.2})",
                        vmaf.score, threshold, vmaf.min_score
                    );
                } else {
                    warn!(
                        "VMAF mean {:.2} and min {:.2} are below threshold {:.2}",
                        vmaf.score, vmaf.min_score, threshold
                    );
                }
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

/// DV profile 5 → HDR10 is a tone-mapping pass: output pixels differ from
/// the source by design and VMAF is not comparable.
fn skips_vmaf(params: &EncodingParams) -> bool {
    params.hdr_type == HdrType::DolbyVision
        && params.dv_profile == Some(5)
        && params.dv_mode == DvMode::ToHdr10
}

#[cfg(test)]
mod tests {
    use super::{
        DvMode, EncodingParams, HdrType, SourceIdentity, VideoMetadata, flush_to_disk, skips_vmaf,
    };
    use crate::config::{AppConfig, Encoder};
    use crate::tracks::OutputTracks;

    #[test]
    fn hardware_encoder_skips_vmaf_for_a_keep_dv_profile5_job() {
        let metadata = VideoMetadata {
            width: 3840,
            height: 2160,
            hdr_type: HdrType::DolbyVision,
            dv_profile: Some(5),
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "hevc".to_string(),
            frame_rate_num: 24000,
            frame_rate_den: 1001,
            duration_secs: 60.0,
        };
        let config = AppConfig {
            encoder: Encoder::Nvenc,
            ..AppConfig::default()
        };
        let params = EncodingParams::from_metadata(
            "in.mkv",
            "out.mkv",
            &metadata,
            &config,
            OutputTracks::default(),
            DvMode::KeepDolbyVision,
            false,
            Vec::new(),
        );
        assert_eq!(params.dv_mode, DvMode::ToHdr10);
        assert!(skips_vmaf(&params));
    }

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

    #[test]
    fn an_output_that_cannot_be_flushed_is_reported() {
        let path = std::env::temp_dir().join(format!("av1c_flush_{}", std::process::id()));
        std::fs::write(&path, b"encoded").unwrap();
        assert!(flush_to_disk(path.to_str().unwrap()).is_ok());
        std::fs::remove_file(&path).unwrap();
        assert!(flush_to_disk(path.to_str().unwrap()).is_err());
    }
}
