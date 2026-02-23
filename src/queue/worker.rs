use crate::analyzer::VideoMetadata;
use crate::config::AppConfig;
use crate::encoder::{self, FullEncodeResult};
use crate::tracks::TrackSelection;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use tracing::info;

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
            FullEncodeResult::SuccessWithVmaf {
                vmaf,
                source_deleted,
            } => {
                let score = vmaf.score;
                if source_deleted {
                    let _ = tx.send(WorkerMessage::SourceDeleted(job.index));
                }
                let _ = tx.send(WorkerMessage::DoneWithVmaf(job.index, score));
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
                info!(
                    "Source file kept: {} (VMAF {:.1} < {:.0})",
                    job.input.display(),
                    score,
                    threshold
                );
                let _ = tx.send(WorkerMessage::SourceKeptLowVmaf(job.index, score));
                let _ = tx.send(WorkerMessage::QualityWarning(job.index, score, threshold));
            }
        }
    }
}
