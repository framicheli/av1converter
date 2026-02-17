use crate::analyzer::VideoMetadata;
use crate::config::AppConfig;
use crate::encoder::{self, FullEncodeResult};
use crate::tracks::TrackSelection;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use tracing::{info, warn};

/// Messages sent from the worker thread to the main thread
pub enum WorkerMessage {
    /// Progress update for a file
    Progress(usize, f32),
    /// Encoding completed successfully
    Done(usize),
    /// Encoding completed with VMAF score
    DoneWithVmaf(usize, f64),
    /// Error occurred
    Error(usize, String),
    /// Quality below threshold
    QualityWarning(usize, f64, f64),
    /// Encoding was cancelled
    Cancelled,
    /// Source file was deleted after successful encoding
    SourceDeleted(usize),
    /// Source file was kept because VMAF was below 90
    SourceKeptLowVmaf(usize, f64),
}

/// Data needed by the worker thread for one job
#[derive(Clone)]
pub struct WorkerJob {
    pub index: usize,
    pub input: PathBuf,
    pub output: PathBuf,
    pub metadata: VideoMetadata,
    pub tracks: TrackSelection,
    pub delete_source: bool,
}

/// Run the encoding worker in a separate thread
pub fn run_worker(
    jobs: Vec<WorkerJob>,
    config: AppConfig,
    cancel_flag: Arc<AtomicBool>,
    tx: Sender<WorkerMessage>,
) {
    for job in jobs {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = tx.send(WorkerMessage::Cancelled);
            break;
        }

        let _ = tx.send(WorkerMessage::Progress(job.index, 0.0));

        let tx_progress = tx.clone();
        let idx = job.index;

        let input_str = job.input.to_str().unwrap_or("").to_string();
        let output_str = job.output.to_str().unwrap_or("").to_string();

        let result = encoder::run_encoding_pipeline(
            &input_str,
            &output_str,
            &job.metadata,
            job.tracks,
            &config,
            Some(Box::new(move |progress| {
                let _ = tx_progress.send(WorkerMessage::Progress(idx, progress));
            })),
            cancel_flag.clone(),
        );

        match result {
            FullEncodeResult::Success => {
                let _ = tx.send(WorkerMessage::Done(job.index));
            }
            FullEncodeResult::SuccessWithVmaf(vmaf) => {
                let score = vmaf.score;
                let _ = tx.send(WorkerMessage::DoneWithVmaf(job.index, score));
                if job.delete_source {
                    try_delete_source(&tx, job.index, &job.input, score);
                }
            }
            FullEncodeResult::Cancelled => {
                let _ = tx.send(WorkerMessage::Cancelled);
                break;
            }
            FullEncodeResult::Error(e) => {
                let _ = tx.send(WorkerMessage::Error(job.index, e));
            }
            FullEncodeResult::QualityWarning { vmaf, threshold } => {
                let score = vmaf.score;
                let _ = tx.send(WorkerMessage::QualityWarning(job.index, score, threshold));
                if job.delete_source {
                    try_delete_source(&tx, job.index, &job.input, score);
                }
            }
        }
    }
}

const DELETE_VMAF_THRESHOLD: f64 = 90.0;

/// Attempt to delete the source file if VMAF score meets the deletion threshold
fn try_delete_source(tx: &Sender<WorkerMessage>, index: usize, source: &PathBuf, vmaf_score: f64) {
    if vmaf_score >= DELETE_VMAF_THRESHOLD {
        match std::fs::remove_file(source) {
            Ok(()) => {
                info!(
                    "Deleted source file: {} (VMAF: {:.1})",
                    source.display(),
                    vmaf_score
                );
                let _ = tx.send(WorkerMessage::SourceDeleted(index));
            }
            Err(e) => {
                warn!("Failed to delete source file {}: {}", source.display(), e);
                let _ = tx.send(WorkerMessage::Error(
                    index,
                    format!("Failed to delete source: {}", e),
                ));
            }
        }
    } else {
        info!(
            "Source file kept: {} (VMAF {:.1} < {:.0})",
            source.display(),
            vmaf_score,
            DELETE_VMAF_THRESHOLD
        );
        let _ = tx.send(WorkerMessage::SourceKeptLowVmaf(index, vmaf_score));
    }
}
