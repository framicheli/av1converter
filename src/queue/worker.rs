use crate::analyzer::{DvMode, VideoMetadata};
use crate::config::AppConfig;
use crate::encoder::{self, FullEncodeResult, KeepReason};
use crate::i18n::{Msg, t};
use crate::queue::SourceIdentity;
use crate::tracks::OutputTracks;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use tracing::info;

/// Messages sent from the worker thread to the main thread
pub enum WorkerMessage {
    /// Progress update for a file (index, percent 0–100)
    Progress(usize, f64),
    /// VMAF quality check is starting for this job
    Verifying(usize),
    /// Encoding completed successfully (no VMAF run)
    Done(usize),
    /// Encoding completed with VMAF score meeting threshold
    DoneWithVmaf(usize, f64),
    /// Encoding succeeded but VMAF check could not run
    DoneVmafFailed(usize, String),
    /// Error occurred
    Error(usize, String),
    /// Quality below threshold (index, mean, min, threshold)
    QualityWarning(usize, f64, f64, f64),
    /// Encoding was cancelled
    Cancelled,
    /// Every job assigned to this worker session has finished
    Finished,
    /// Source file was deleted after successful encoding
    SourceDeleted(usize),
    /// Source file was kept: VMAF mean and/or min was below the threshold
    /// (index, mean, min)
    SourceKeptLowVmaf(usize, f64, f64),
    /// Source file was kept although VMAF met the threshold
    SourceKept(usize, KeepReason),
}

/// Data needed by the worker thread for one job
#[derive(Clone)]
pub struct WorkerJob {
    pub index: usize,
    pub input: PathBuf,
    pub output: PathBuf,
    pub source_identity: SourceIdentity,
    pub metadata: VideoMetadata,
    /// Audio and subtitle streams to write, already resolved from the user's
    /// selection into output order
    pub tracks: OutputTracks,
    pub dv_mode: DvMode,
    pub remux_only: bool,
    /// Per-selected-track subtitle codecs (`copy`, a compatible conversion, or
    /// `None` for a track the container cannot hold).
    pub subtitle_codecs: Vec<Option<&'static str>>,
}

/// Run the encoding worker in a separate thread
#[allow(clippy::too_many_lines)]
pub fn run_worker(
    jobs: Vec<WorkerJob>,
    config: &AppConfig,
    cancel_flag: &Arc<AtomicBool>,
    tx: &Sender<WorkerMessage>,
) {
    for job in jobs {
        if cancel_flag.load(std::sync::atomic::Ordering::Acquire) {
            let _ = tx.send(WorkerMessage::Cancelled);
            return;
        }
        let _ = tx.send(WorkerMessage::Progress(job.index, 0.0));

        let tx_progress = tx.clone();
        let idx = job.index;

        // The pipeline takes `&str` paths; a non-UTF-8 path is reported as a
        // job error.
        let (Some(input_str), Some(output_str)) = (job.input.to_str(), job.output.to_str()) else {
            let _ = tx.send(WorkerMessage::Error(
                job.index,
                t(config.language, Msg::ErrPathNotUtf8)
                    .replace("{path}", &job.input.display().to_string()),
            ));
            continue;
        };
        let input_str = input_str.to_string();
        let output_str = output_str.to_string();

        let tx_verifying = tx.clone();
        let verifying_idx = job.index;

        // A pipeline panic becomes a per-job error and the session continues.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            encoder::run_encoding_pipeline(
                &input_str,
                &output_str,
                &job.source_identity,
                &job.metadata,
                job.tracks,
                job.dv_mode,
                job.remux_only,
                job.subtitle_codecs,
                config,
                Some(Box::new(move |progress| {
                    let _ = tx_progress.send(WorkerMessage::Progress(idx, progress));
                })),
                cancel_flag.as_ref(),
                Some(Box::new(move || {
                    let _ = tx_verifying.send(WorkerMessage::Verifying(verifying_idx));
                })),
            )
        }))
        .unwrap_or_else(|_| {
            FullEncodeResult::Error(
                t(config.language, Msg::ErrEncodePanicked)
                    .replace("{path}", &job.input.display().to_string()),
            )
        });

        if !report_result(tx, job.index, &job.input, result, config) {
            return;
        }
    }
    let _ = tx.send(WorkerMessage::Finished);
}

/// Turn one pipeline result into the messages the UIs read. Returns `false`
/// when the session was cancelled and no further job runs.
fn report_result(
    tx: &Sender<WorkerMessage>,
    index: usize,
    input: &std::path::Path,
    result: FullEncodeResult,
    config: &AppConfig,
) -> bool {
    match result {
        FullEncodeResult::Success => {
            let _ = tx.send(WorkerMessage::Done(index));
        }
        FullEncodeResult::SuccessWithVmaf {
            vmaf,
            source_deleted,
            keep_reason,
        } => {
            let score = vmaf.score;
            if source_deleted {
                let _ = tx.send(WorkerMessage::SourceDeleted(index));
            }
            if let Some(reason) = keep_reason {
                let _ = tx.send(WorkerMessage::SourceKept(index, reason));
            }
            let _ = tx.send(WorkerMessage::DoneWithVmaf(index, score));
        }
        FullEncodeResult::VmafFailed { message } => {
            let _ = tx.send(WorkerMessage::DoneVmafFailed(index, message));
        }
        FullEncodeResult::Cancelled => {
            let _ = tx.send(WorkerMessage::Cancelled);
            return false;
        }
        FullEncodeResult::Error(e) => {
            let _ = tx.send(WorkerMessage::Error(index, e));
        }
        FullEncodeResult::QualityWarning { vmaf, threshold } => {
            if config.quality.delete_source_on_success {
                info!(
                    "Source file kept: {} (VMAF mean {:.1}, min {:.1}, threshold {})",
                    input.display(),
                    vmaf.score,
                    vmaf.min_score,
                    threshold
                );
            }
            report_low_vmaf(
                tx,
                index,
                vmaf.score,
                vmaf.min_score,
                threshold,
                config.quality.delete_source_on_success,
            );
        }
    }
    true
}

/// Report a job whose VMAF mean or minimum is below `threshold`: a
/// [`WorkerMessage::SourceKeptLowVmaf`] when source deletion is enabled, then
/// a [`WorkerMessage::QualityWarning`].
fn report_low_vmaf(
    tx: &Sender<WorkerMessage>,
    index: usize,
    mean: f64,
    min: f64,
    threshold: f64,
    deletion_enabled: bool,
) {
    if deletion_enabled {
        let _ = tx.send(WorkerMessage::SourceKeptLowVmaf(index, mean, min));
    }
    let _ = tx.send(WorkerMessage::QualityWarning(index, mean, min, threshold));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_worker_session_reports_completion() {
        let (tx, rx) = std::sync::mpsc::channel();
        run_worker(
            Vec::new(),
            &AppConfig::default(),
            &Arc::new(AtomicBool::new(false)),
            &tx,
        );

        assert!(matches!(rx.recv().unwrap(), WorkerMessage::Finished));
    }

    /// Each pipeline result turns into its own messages, in order, and only
    /// a cancellation ends the session.
    #[test]
    fn every_pipeline_result_maps_to_its_messages() {
        use crate::encoder::KeepReason;
        use crate::verifier::VmafResult;

        let vmaf = VmafResult {
            score: 97.0,
            min_score: 96.0,
            max_score: 99.0,
        };
        let config = AppConfig::default();
        let report = |result| {
            let (tx, rx) = std::sync::mpsc::channel();
            let carry_on = report_result(&tx, 7, std::path::Path::new("in.mkv"), result, &config);
            drop(tx);
            (carry_on, rx.iter().collect::<Vec<WorkerMessage>>())
        };

        let (carry_on, messages) = report(FullEncodeResult::Success);
        assert!(carry_on);
        assert!(matches!(messages[..], [WorkerMessage::Done(7)]));

        let (carry_on, messages) = report(FullEncodeResult::SuccessWithVmaf {
            vmaf: vmaf.clone(),
            source_deleted: true,
            keep_reason: None,
        });
        assert!(carry_on);
        assert!(matches!(
            messages[..],
            [
                WorkerMessage::SourceDeleted(7),
                WorkerMessage::DoneWithVmaf(7, 97.0)
            ]
        ));

        let (_, messages) = report(FullEncodeResult::SuccessWithVmaf {
            vmaf: vmaf.clone(),
            source_deleted: false,
            keep_reason: Some(KeepReason::Symlink),
        });
        assert!(matches!(
            messages[..],
            [
                WorkerMessage::SourceKept(7, KeepReason::Symlink),
                WorkerMessage::DoneWithVmaf(7, 97.0)
            ]
        ));

        let (_, messages) = report(FullEncodeResult::VmafFailed {
            message: "no libvmaf".to_string(),
        });
        assert!(
            matches!(&messages[..], [WorkerMessage::DoneVmafFailed(7, message)] if message == "no libvmaf")
        );

        let (_, messages) = report(FullEncodeResult::Error("boom".to_string()));
        assert!(matches!(&messages[..], [WorkerMessage::Error(7, e)] if e == "boom"));

        let (carry_on, messages) = report(FullEncodeResult::Cancelled);
        assert!(!carry_on, "a cancellation ends the session");
        assert!(matches!(messages[..], [WorkerMessage::Cancelled]));

        // With deletion off, only the warning is sent.
        let (carry_on, messages) = report(FullEncodeResult::QualityWarning {
            vmaf: vmaf.clone(),
            threshold: 98.0,
        });
        assert!(carry_on);
        assert!(matches!(
            messages[..],
            [WorkerMessage::QualityWarning(7, 97.0, 96.0, 98.0)]
        ));
    }

    #[test]
    fn a_low_vmaf_reports_the_kept_source_only_when_deletion_is_on() {
        let (tx, rx) = std::sync::mpsc::channel();
        report_low_vmaf(&tx, 3, 96.0, 80.0, 95.0, true);
        report_low_vmaf(&tx, 4, 96.0, 80.0, 95.0, false);
        drop(tx);
        let messages: Vec<WorkerMessage> = rx.iter().collect();

        assert_eq!(messages.len(), 3);
        assert!(matches!(
            messages[0],
            WorkerMessage::SourceKeptLowVmaf(3, 96.0, 80.0)
        ));
        assert!(matches!(
            messages[1],
            WorkerMessage::QualityWarning(3, 96.0, 80.0, 95.0)
        ));
        assert!(matches!(
            messages[2],
            WorkerMessage::QualityWarning(4, 96.0, 80.0, 95.0)
        ));
    }
}
