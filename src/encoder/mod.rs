use crate::i18n::{Msg, t};
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
        /// Why the source was kept although the score met the threshold.
        keep_reason: Option<KeepReason>,
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

/// Why a job whose VMAF met the threshold still kept its source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeepReason {
    /// Dolby Vision was to be kept but the encoder converted it to HDR10.
    DolbyVisionConverted,
    /// Audio was transcoded and VMAF does not verify it.
    AudioTranscoded,
    /// A selected subtitle track was converted or left out.
    SubtitleChanged,
    /// The Dolby Vision profile 7 enhancement layer is not carried over.
    DolbyVisionProfile7,
    /// The source chroma or bit depth exceeds the 4:2:0 10-bit encode.
    ChromaOrBitDepth,
    /// The source streams could not be checked.
    StreamsUnchecked,
    /// The source has video or data streams the output does not carry.
    ExtraStreams,
    /// The source cover art or attachments are not carried over.
    Attachments,
    /// Cancellation was requested.
    Cancelled,
    /// The encoded output changed before deletion.
    OutputChanged,
    /// The encoded output could not be flushed to disk.
    FlushFailed,
    /// The source is a symbolic link.
    Symlink,
    /// The source changed while the job was running.
    SourceChanged,
    /// Deleting the source failed.
    DeleteFailed,
    /// The deletion ledger could not be written.
    LedgerUnwritable,
}

impl KeepReason {
    /// The English clause used in the log line.
    pub fn log(self) -> &'static str {
        match self {
            Self::DolbyVisionConverted => {
                "Dolby Vision was to be kept but this encoder converted it to HDR10"
            }
            Self::AudioTranscoded => "audio was transcoded and VMAF does not verify it",
            Self::SubtitleChanged => "a selected subtitle track was converted or left out",
            Self::DolbyVisionProfile7 => {
                "its Dolby Vision profile 7 enhancement layer is not carried into the output"
            }
            Self::ChromaOrBitDepth => {
                "its chroma or bit depth may be reduced by the 4:2:0 10-bit encode"
            }
            Self::StreamsUnchecked => "its streams could not be checked",
            Self::ExtraStreams => "it has video or data streams the output does not carry",
            Self::Attachments => "its cover art or attachments are not carried into the output",
            Self::Cancelled => "cancellation was requested",
            Self::OutputChanged => "the encoded output changed before deletion",
            Self::FlushFailed => "the encoded output could not be flushed to disk",
            Self::Symlink => "it is a symbolic link",
            Self::SourceChanged => "it changed while the job was running",
            Self::DeleteFailed => "deleting it failed",
            Self::LedgerUnwritable => "the deletion ledger could not be written",
        }
    }

    /// The translated message shown in the UIs.
    pub fn msg(self) -> crate::i18n::Msg {
        use crate::i18n::Msg;
        match self {
            Self::DolbyVisionConverted => Msg::KeepDvConverted,
            Self::AudioTranscoded => Msg::KeepAudioTranscoded,
            Self::SubtitleChanged => Msg::KeepSubtitleChanged,
            Self::DolbyVisionProfile7 => Msg::KeepDvProfile7,
            Self::ChromaOrBitDepth => Msg::KeepChromaOrBitDepth,
            Self::StreamsUnchecked => Msg::KeepStreamsUnchecked,
            Self::ExtraStreams => Msg::KeepExtraStreams,
            Self::Attachments => Msg::KeepAttachments,
            Self::Cancelled => Msg::KeepCancelled,
            Self::OutputChanged => Msg::KeepOutputChanged,
            Self::FlushFailed => Msg::KeepFlushFailed,
            Self::Symlink => Msg::KeepSymlink,
            Self::SourceChanged => Msg::KeepSourceChanged,
            Self::DeleteFailed => Msg::KeepDeleteFailed,
            Self::LedgerUnwritable => Msg::KeepLedgerUnwritable,
        }
    }
}

/// One line of the deletion ledger.
#[derive(serde::Serialize)]
struct DeletionRecord<'a> {
    /// Seconds since the Unix epoch.
    time: u64,
    source: &'a str,
    output: &'a str,
    vmaf_mean: f64,
    vmaf_min: f64,
    threshold: f64,
    source_bytes: Option<u64>,
    output_bytes: Option<u64>,
    /// `deleted` or `kept`.
    action: &'static str,
    reason: Option<KeepReason>,
}

impl<'a> DeletionRecord<'a> {
    /// The decision taken for `source` against `vmaf`, with the sizes read
    /// from disk as they stand.
    fn new(
        source: &'a str,
        output: &'a str,
        vmaf: &verifier::VmafResult,
        threshold: f64,
        reason: Option<KeepReason>,
    ) -> Self {
        let size = |path: &str| std::fs::metadata(path).ok().map(|m| m.len());
        Self {
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
            source,
            output,
            vmaf_mean: vmaf.score,
            vmaf_min: vmaf.min_score,
            threshold,
            source_bytes: size(source),
            output_bytes: size(output),
            action: if reason.is_some() { "kept" } else { "deleted" },
            reason,
        }
    }
}

/// The append-only ledger of deletion decisions.
fn ledger_path() -> std::path::PathBuf {
    ledger_dir().join("deletions.jsonl")
}

#[cfg(not(test))]
fn ledger_dir() -> std::path::PathBuf {
    crate::daemon::lifecycle::data_dir()
}

/// A ledger directory of its own per test thread.
#[cfg(test)]
fn ledger_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "av1c_ledger_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

/// Append `record` as one JSON line to the ledger and flush it to disk.
fn append_ledger(record: &DeletionRecord) -> std::io::Result<()> {
    use std::io::Write;

    crate::utils::ensure_private_dir(&ledger_dir())?;
    let mut line = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger_path())?;
    file.write_all(&line)?;
    file.sync_data()
}

/// Record the deletion of `source` and remove it. Returns the reason the
/// source was kept instead: an unwritable ledger, or a removal that failed.
fn record_and_delete(
    source: &str,
    output: &str,
    vmaf: &verifier::VmafResult,
    threshold: f64,
) -> Option<KeepReason> {
    if let Err(e) = append_ledger(&DeletionRecord::new(source, output, vmaf, threshold, None)) {
        warn!(
            "Keeping source file {source}: {}: {e}",
            KeepReason::LedgerUnwritable.log()
        );
        return Some(KeepReason::LedgerUnwritable);
    }
    match std::fs::remove_file(source) {
        Ok(()) => None,
        Err(e) => {
            warn!("Failed to delete source file {source}: {e}");
            Some(KeepReason::DeleteFailed)
        }
    }
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
            t(config.language, Msg::ErrSourceChangedSinceAnalysis).to_string(),
        );
    }
    // The original stays open and fingerprinted for the whole pipeline; a file
    // that replaces it at the same path no longer matches.
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
                    t(config.language, Msg::ErrSourceChangedWhilePreparing).to_string(),
                );
            }
            Err(e) => {
                return FullEncodeResult::Error(
                    t(config.language, Msg::ErrSourceHoldFailed).replace("{error}", &e.to_string()),
                );
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
            // The file the encoder placed at the output path is remembered
            // and re-checked before VMAF and source deletion.
            let output_identity = config
                .quality
                .delete_source_on_success
                .then(|| SourceIdentity::from_path(output).ok())
                .flatten();
            let tone_mapped = skips_vmaf(&params);
            if tone_mapped && config.quality.vmaf_enabled {
                info!("Skipping VMAF: DV profile 5 tone-mapped output is not comparable");
            }

            // A cancel between the ffmpeg exit and verification removes the
            // finished output.
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
                    ref vmaf,
                    ref mut source_deleted,
                    ref mut keep_reason,
                } = result
                {
                    let threshold = config.quality.vmaf_threshold;
                    if cancel_flag.load(std::sync::atomic::Ordering::Acquire) {
                        warn!(
                            "Keeping source file {input}: {}",
                            KeepReason::Cancelled.log()
                        );
                        *keep_reason = Some(KeepReason::Cancelled);
                    } else if let Some(reason) = keep_source_reason(&params, dv_mode, cancel_flag) {
                        info!("Keeping source file {input}: {}", reason.log());
                        *keep_reason = Some(reason);
                    } else if !output_identity
                        .as_ref()
                        .is_some_and(|identity| identity.matches_path(output))
                    {
                        warn!(
                            "Keeping source file {input}: {}",
                            KeepReason::OutputChanged.log()
                        );
                        *keep_reason = Some(KeepReason::OutputChanged);
                    } else if let Err(e) = flush_to_disk(output) {
                        warn!(
                            "Keeping source file {input}: {}: {e}",
                            KeepReason::FlushFailed.log()
                        );
                        *keep_reason = Some(KeepReason::FlushFailed);
                    } else if std::fs::symlink_metadata(input)
                        .is_ok_and(|m| m.file_type().is_symlink())
                    {
                        warn!("Keeping source file {input}: {}", KeepReason::Symlink.log());
                        *keep_reason = Some(KeepReason::Symlink);
                    } else if !expected_source.matches_path(input) {
                        warn!(
                            "Keeping source file {input}: {}",
                            KeepReason::SourceChanged.log()
                        );
                        *keep_reason = Some(KeepReason::SourceChanged);
                    } else {
                        match record_and_delete(input, output, vmaf, threshold) {
                            None => {
                                info!("Deleted source file: {input}");
                                *source_deleted = true;
                            }
                            Some(reason) => *keep_reason = Some(reason),
                        }
                    }

                    // Every decision that kept the source is recorded as well.
                    // A ledger that could not be written cannot record that.
                    if let Some(reason) = *keep_reason
                        && reason != KeepReason::LedgerUnwritable
                        && let Err(e) = append_ledger(&DeletionRecord::new(
                            input,
                            output,
                            vmaf,
                            threshold,
                            Some(reason),
                        ))
                    {
                        warn!("Could not record the kept source file {input}: {e}");
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
/// whatever the score: Dolby Vision that was asked to be kept but is not,
/// transcoded audio, subtitles converted or left out, a Dolby Vision profile 7
/// enhancement layer, a source pixel format beyond 4:2:0 at 10 bits or one
/// that cannot be read, extra video or data streams, and cover art or
/// attachments the output container does not carry.
fn keep_source_reason(
    params: &EncodingParams,
    requested_dv_mode: DvMode,
    cancel: &AtomicBool,
) -> Option<KeepReason> {
    if params.hdr_type == HdrType::DolbyVision
        && requested_dv_mode == DvMode::KeepDolbyVision
        && params.dv_mode != DvMode::KeepDolbyVision
    {
        return Some(KeepReason::DolbyVisionConverted);
    }
    if params.tracks.transcodes_audio() {
        return Some(KeepReason::AudioTranscoded);
    }
    if params.subtitle_codecs.len() != params.tracks.subtitle_indices.len()
        || params
            .subtitle_codecs
            .iter()
            .any(|codec| *codec != Some("copy"))
    {
        return Some(KeepReason::SubtitleChanged);
    }
    if params.hdr_type == HdrType::DolbyVision && params.dv_profile == Some(7) {
        return Some(KeepReason::DolbyVisionProfile7);
    }
    if !crate::analyzer::ffprobe::probe_video_pix_fmt(&params.input, cancel)
        .is_some_and(|pix_fmt| is_yuv420_within_10_bit(&pix_fmt))
    {
        return Some(KeepReason::ChromaOrBitDepth);
    }
    match crate::analyzer::ffprobe::probe_unselectable_streams(&params.input, cancel) {
        None => Some(KeepReason::StreamsUnchecked),
        Some(streams) if streams.other > 0 => Some(KeepReason::ExtraStreams),
        Some(streams)
            if streams.pictures == 0
                && (streams.attachments == 0
                    || command_builder::keeps_attachments(&params.output)) =>
        {
            None
        }
        Some(_) => Some(KeepReason::Attachments),
    }
}

/// Whether `pix_fmt` is 4:2:0 at 10 bits per sample or fewer.
fn is_yuv420_within_10_bit(pix_fmt: &str) -> bool {
    matches!(
        pix_fmt,
        "yuv420p"
            | "yuvj420p"
            | "yuv420p9le"
            | "yuv420p9be"
            | "yuv420p10le"
            | "yuv420p10be"
            | "nv12"
            | "nv21"
            | "p010le"
            | "p010be"
    )
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
        // Cancel during verification: the finished output is removed and
        // Cancelled is reported.
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
                keep_reason: None,
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

/// DV profile 5 → HDR10 is a tone-mapping pass: the output pixels differ from
/// the source and VMAF is not comparable.
fn skips_vmaf(params: &EncodingParams) -> bool {
    params.hdr_type == HdrType::DolbyVision
        && params.dv_profile == Some(5)
        && params.dv_mode == DvMode::ToHdr10
}

#[cfg(test)]
mod tests {
    use super::verifier;
    use super::{
        DvMode, EncodingParams, HdrType, KeepReason, SourceIdentity, VideoMetadata, flush_to_disk,
        is_yuv420_within_10_bit, keep_source_reason, skips_vmaf,
    };
    use crate::config::{AppConfig, Encoder};
    use crate::tracks::OutputTracks;

    /// Every keep reason carries its own English log clause and its own
    /// translated message.
    #[test]
    fn each_keep_reason_maps_to_its_own_message() {
        let all = [
            KeepReason::DolbyVisionConverted,
            KeepReason::AudioTranscoded,
            KeepReason::SubtitleChanged,
            KeepReason::DolbyVisionProfile7,
            KeepReason::ChromaOrBitDepth,
            KeepReason::StreamsUnchecked,
            KeepReason::ExtraStreams,
            KeepReason::Attachments,
            KeepReason::Cancelled,
            KeepReason::OutputChanged,
            KeepReason::FlushFailed,
            KeepReason::Symlink,
            KeepReason::SourceChanged,
            KeepReason::DeleteFailed,
        ];
        let mut texts: Vec<&'static str> = all
            .iter()
            .map(|reason| crate::i18n::t(crate::i18n::Language::English, reason.msg()))
            .collect();
        texts.sort_unstable();
        let count = texts.len();
        texts.dedup();
        assert_eq!(texts.len(), count);

        assert_eq!(
            crate::i18n::t(
                crate::i18n::Language::English,
                KeepReason::FlushFailed.msg()
            ),
            "the encoded output could not be flushed to disk"
        );
        assert_eq!(
            KeepReason::AudioTranscoded.log(),
            "audio was transcoded and VMAF does not verify it"
        );
    }

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
    fn a_keep_dv_request_a_hardware_encoder_cannot_honour_keeps_the_source() {
        let metadata = VideoMetadata {
            width: 3840,
            height: 2160,
            hdr_type: HdrType::DolbyVision,
            dv_profile: Some(8),
            dv_bl_compat: Some(1),
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
            "/nonexistent/in.mkv",
            "/nonexistent/out.mkv",
            &metadata,
            &config,
            OutputTracks::default(),
            DvMode::KeepDolbyVision,
            false,
            Vec::new(),
        );
        assert_eq!(
            keep_source_reason(
                &params,
                DvMode::KeepDolbyVision,
                &std::sync::atomic::AtomicBool::new(false)
            ),
            Some(KeepReason::DolbyVisionConverted)
        );
    }

    #[test]
    fn a_dolby_vision_profile_7_source_is_kept() {
        let metadata = VideoMetadata {
            width: 3840,
            height: 2160,
            hdr_type: HdrType::DolbyVision,
            dv_profile: Some(7),
            dv_bl_compat: Some(6),
            hdr10_static: None,
            codec_name: "hevc".to_string(),
            frame_rate_num: 24000,
            frame_rate_den: 1001,
            duration_secs: 60.0,
        };
        let params = EncodingParams::from_metadata(
            "/nonexistent/in.mkv",
            "/nonexistent/out.mkv",
            &metadata,
            &AppConfig::default(),
            OutputTracks::default(),
            DvMode::ToHdr10,
            false,
            Vec::new(),
        );
        assert_eq!(
            keep_source_reason(
                &params,
                DvMode::ToHdr10,
                &std::sync::atomic::AtomicBool::new(false)
            ),
            Some(KeepReason::DolbyVisionProfile7)
        );
    }

    /// Subtitle indices and codecs out of step, as after a hand-edited queue
    /// file, keep the source.
    #[test]
    fn subtitle_indices_without_matching_codecs_keep_the_source() {
        let metadata = VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: HdrType::Sdr,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "h264".to_string(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 60.0,
        };
        let tracks = OutputTracks {
            subtitle_indices: vec![0, 1],
            ..OutputTracks::default()
        };
        let params = EncodingParams::from_metadata(
            "/nonexistent/in.mkv",
            "/nonexistent/out.mkv",
            &metadata,
            &AppConfig::default(),
            tracks,
            DvMode::ToHdr10,
            false,
            vec![Some("copy")],
        );
        assert_eq!(
            keep_source_reason(
                &params,
                DvMode::ToHdr10,
                &std::sync::atomic::AtomicBool::new(false)
            ),
            Some(KeepReason::SubtitleChanged)
        );
    }

    #[test]
    fn only_4_2_0_formats_up_to_10_bit_count_as_carried() {
        assert!(is_yuv420_within_10_bit("yuv420p"));
        assert!(is_yuv420_within_10_bit("yuv420p10le"));
        assert!(!is_yuv420_within_10_bit("yuv422p10le"));
        assert!(!is_yuv420_within_10_bit("yuv444p"));
        assert!(!is_yuv420_within_10_bit("yuv420p12le"));
        assert!(!is_yuv420_within_10_bit("yuva420p"));
        assert!(!is_yuv420_within_10_bit("gbrp"));
        assert!(!is_yuv420_within_10_bit(""));
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
        drop(original);

        // A different length tells the replacement apart on every platform;
        // Unix also sees the new inode.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"a replacement").unwrap();
        assert!(!identity.matches_path(path.to_str().unwrap()));

        let _ = std::fs::remove_dir_all(dir);
    }

    /// A scratch directory holding a source and an output file, with this
    /// thread's ledger removed.
    fn ledger_fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let _ = std::fs::remove_dir_all(super::ledger_dir());
        let dir =
            std::env::temp_dir().join(format!("av1c_ledger_job_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("movie.mkv");
        let output = dir.join("movie_av1.mkv");
        std::fs::write(&source, b"source bytes").unwrap();
        std::fs::write(&output, b"out").unwrap();
        (source, output)
    }

    /// The ledger's only line, parsed.
    fn only_ledger_line() -> serde_json::Value {
        let contents = std::fs::read_to_string(super::ledger_path()).unwrap();
        let mut lines = contents.lines();
        let line = lines.next().expect("the ledger holds a line");
        assert_eq!(lines.next(), None, "one line per decision");
        serde_json::from_str(line).unwrap()
    }

    fn passing_score() -> verifier::VmafResult {
        verifier::VmafResult {
            score: 96.5,
            min_score: 95.25,
            max_score: 99.0,
        }
    }

    #[test]
    fn a_deleted_source_is_recorded_as_one_ledger_line() {
        let (source, output) = ledger_fixture("deleted");
        let (source, output) = (
            source.to_str().unwrap().to_string(),
            output.to_str().unwrap().to_string(),
        );

        assert_eq!(
            super::record_and_delete(&source, &output, &passing_score(), 95.0),
            None
        );
        assert!(!std::path::Path::new(&source).exists());

        let line = only_ledger_line();
        assert_eq!(line["source"], serde_json::json!(source));
        assert_eq!(line["output"], serde_json::json!(output));
        assert_eq!(line["action"], serde_json::json!("deleted"));
        assert_eq!(line["reason"], serde_json::Value::Null);
        assert_eq!(line["vmaf_mean"], serde_json::json!(96.5));
        assert_eq!(line["vmaf_min"], serde_json::json!(95.25));
        assert_eq!(line["threshold"], serde_json::json!(95.0));
        assert_eq!(line["source_bytes"], serde_json::json!(12));
        assert_eq!(line["output_bytes"], serde_json::json!(3));
        assert!(line["time"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(super::ledger_dir());
    }

    #[test]
    fn a_kept_source_is_recorded_with_its_reason() {
        let (source, output) = ledger_fixture("kept");
        let record = super::DeletionRecord::new(
            source.to_str().unwrap(),
            output.to_str().unwrap(),
            &passing_score(),
            95.0,
            Some(KeepReason::AudioTranscoded),
        );
        super::append_ledger(&record).unwrap();

        let line = only_ledger_line();
        assert_eq!(line["action"], serde_json::json!("kept"));
        assert_eq!(line["reason"], serde_json::json!("AudioTranscoded"));
        assert!(source.exists());
        let _ = std::fs::remove_dir_all(super::ledger_dir());
    }

    #[test]
    fn a_ledger_that_cannot_be_written_keeps_the_source() {
        let (source, output) = ledger_fixture("unwritable");
        // A directory at the ledger's path cannot be opened as a file.
        crate::utils::ensure_private_dir(&super::ledger_path()).unwrap();

        assert_eq!(
            super::record_and_delete(
                source.to_str().unwrap(),
                output.to_str().unwrap(),
                &passing_score(),
                95.0,
            ),
            Some(KeepReason::LedgerUnwritable)
        );
        assert!(source.exists(), "an unrecorded deletion keeps the source");
        let _ = std::fs::remove_dir_all(super::ledger_dir());
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
