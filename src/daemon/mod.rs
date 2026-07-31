pub mod api;
pub mod lifecycle;
pub mod server;
pub mod state;

use crate::analyzer::{self, AnalysisResult, DvMode, HdrType, is_av1_codec};
use crate::config::{AppConfig, AudioMode, Encoder};
use crate::error::AppError;
use crate::i18n::{Msg, t};
use crate::queue::{
    EncodingJob, JobStatus, WorkerJob, WorkerMessage, auto_select_tracks, make_output_paths_unique,
    run_worker,
};
use crate::utils::DependencyStatus;
use state::{DaemonState, EncodeSession, SharedState, is_terminal, lock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// How often the orchestrator wakes up when no analysis result arrives.
const TICK: Duration = Duration::from_millis(250);
/// How long shutdown waits for the worker to acknowledge cancellation.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
/// Requests are served concurrently: a recursive scan or a listing of a slow
/// network mount would otherwise stall the dashboard poll behind it.
const SERVER_THREADS: usize = 4;

/// Run the headless daemon: web server + encoding orchestrator.
/// Blocks until SIGINT/SIGTERM.
pub fn run_daemon(config: AppConfig) -> Result<(), AppError> {
    if !DependencyStatus::check() {
        warn!("ffmpeg or ffprobe was not found on PATH; encoding will fail");
    }
    if config.quality.vmaf_enabled && !DependencyStatus::vmaf_available() {
        warn!("VMAF is enabled but this FFmpeg build has no libvmaf; verification will fail");
    }
    if config.audio.default_mode == AudioMode::Opus && !DependencyStatus::libopus_available() {
        warn!("Audio is set to Opus but this FFmpeg build has no libopus; encoding will fail");
    }
    let encoder_name = config.encoder.ffmpeg_name();
    if !DependencyStatus::encoder_available(encoder_name) {
        println!(
            "{} ({encoder_name})",
            t(config.language, Msg::EncoderUnavailable)
        );
        warn!("This FFmpeg build has no {encoder_name}; every encode will fail");
    }

    let lang = config.language;
    let listen = config.daemon.listen_address();

    // Anyone who can reach the port can queue encodes, rewrite the settings and
    // browse the filesystem, so say so plainly when that is more than this host.
    if config.daemon.binds_publicly() && config.daemon.auth_token.is_empty() {
        println!("{}", t(lang, Msg::DaemonPublicNoToken));
        warn!("Daemon is reachable from the network with no auth_token set");
    }

    let shared: SharedState = Arc::new(Mutex::new(DaemonState::new(config)));
    let (analysis_tx, analysis_rx) = mpsc::channel::<(u64, Result<AnalysisResult, AppError>)>();
    let (probe_tx, probe_rx) = mpsc::channel::<(u64, String)>();
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();

    // One long-lived prober rather than a thread per add request: each probe
    // forks ffprobe of its own, and a client adding several folders at once
    // must not be able to decide how many of those run at a time.
    //
    // Because it is shared, it must not be possible to kill: a panic on one
    // malformed file would otherwise take analysis down for the rest of the
    // daemon's life, leaving every later file stuck in `Analyzing` with nothing
    // reported. The panic is caught, blamed on the file that caused it, and the
    // next one is picked up.
    {
        let analysis_tx = analysis_tx.clone();
        thread::spawn(move || {
            for (id, path) in probe_rx {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    analyzer::analyze(&path)
                }))
                .unwrap_or_else(|_| {
                    Err(AppError::Analysis(format!("Analysis panicked on {path}")))
                });
                if analysis_tx.send((id, result)).is_err() {
                    break;
                }
            }
        });
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown = shutdown.clone();
        ctrlc::set_handler(move || {
            if shutdown.swap(true, Ordering::SeqCst) {
                // Second signal: force exit
                std::process::exit(1);
            }
        })
        .map_err(|e| AppError::CommandExecution(format!("Failed to set signal handler: {e}")))?;
    }

    let server = Arc::new(server::bind(&listen)?);
    // Deliberately the bare address, not `url()`: in background mode this
    // process's stdout is the daemon log file, and the token has no business
    // being written there. The tokenised URL is printed to the terminal by
    // whoever started us.
    println!("{} http://{listen}", t(lang, Msg::DaemonListening));
    info!("Web UI listening on http://{listen}");
    let server_handles: Vec<_> = (0..SERVER_THREADS)
        .map(|_| {
            let server = server.clone();
            let shared = shared.clone();
            let probe_tx = probe_tx.clone();
            let shutdown = shutdown.clone();
            thread::spawn(move || server::serve(&server, &shared, &probe_tx, &shutdown))
        })
        .collect();

    // Main orchestrator loop: owns analysis and worker result application.
    while !shutdown.load(Ordering::SeqCst) {
        match analysis_rx.recv_timeout(TICK) {
            Ok((id, result)) => apply_analysis_result(&shared, id, result),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok((id, result)) = analysis_rx.try_recv() {
            apply_analysis_result(&shared, id, result);
        }

        while let Ok(msg) = worker_rx.try_recv() {
            apply_worker_message(&shared, msg);
        }

        maybe_start_session(&shared, &worker_tx);
    }

    // Graceful shutdown: cancel any running encode and wait for the worker
    // to kill ffmpeg and acknowledge.
    let cancelling = {
        let state = lock(&shared);
        if let Some(session) = state.session.as_ref().filter(|_| state.encoding_active) {
            session.cancel_flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    };
    if cancelling {
        println!("{}", t(lang, Msg::DaemonShuttingDown));
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline {
            match worker_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(msg) => {
                    let done = matches!(msg, WorkerMessage::Cancelled);
                    apply_worker_message(&shared, msg);
                    if done {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    for handle in server_handles {
        let _ = handle.join();
    }
    Ok(())
}

/// Queue new files and hand them to the prober.
fn add_paths(
    shared: &SharedState,
    probe_tx: &Sender<(u64, String)>,
    paths: Vec<std::path::PathBuf>,
) -> (usize, usize) {
    let requested = paths.len();
    let mut added = 0;
    let mut to_analyze: Vec<(u64, String)> = Vec::new();
    {
        let mut state = lock(shared);
        // Paths already queued and not yet finished are skipped
        let mut existing: std::collections::HashSet<std::path::PathBuf> = state
            .queue
            .jobs_with_ids()
            .filter(|(_, job)| !is_terminal(&job.status))
            .map(|(_, job)| job.path.canonicalize().unwrap_or_else(|_| job.path.clone()))
            .collect();

        for path in paths {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !existing.insert(canonical) {
                continue;
            }
            let mut job = EncodingJob::new(path);
            if let Some(p) = job.path.to_str() {
                let p = p.to_string();
                job.status = JobStatus::Analyzing;
                let id = state.queue.push(job);
                to_analyze.push((id, p));
                added += 1;
            } else {
                job.status = JobStatus::Error {
                    message: "File path contains non-UTF-8 characters".to_string(),
                };
                state.queue.state.error_count += 1;
                state.queue.push(job);
                added += 1;
            }
        }
    }

    // A job handed over is a job that will be reported on. If the prober is
    // gone the queue must say so, rather than leaving jobs in `Analyzing`
    // forever — a status that never resolves and, being non-terminal, would go
    // on blocking every future attempt to add the same file.
    let mut orphaned: Vec<u64> = Vec::new();
    let mut requests = to_analyze.into_iter();
    for request in requests.by_ref() {
        if let Err(e) = probe_tx.send(request) {
            orphaned.push(e.0.0);
            break;
        }
    }
    orphaned.extend(requests.map(|(id, _)| id));

    if !orphaned.is_empty() {
        warn!(
            "Analysis is not running; {} file(s) rejected",
            orphaned.len()
        );
        let mut state = lock(shared);
        for id in orphaned {
            if let Some(job) = state.queue.job_by_id_mut(id) {
                job.status = JobStatus::Error {
                    message: "Analysis is not running".to_string(),
                };
                state.queue.state.error_count += 1;
                added -= 1;
            }
        }
    }

    (added, requested - added)
}

/// Apply one finished analysis: mirror the TUI's `apply_analysis_results`,
/// then resolve the Dolby Vision mode non-interactively and wait for the WebUI
/// track confirmation.
fn apply_analysis_result(shared: &SharedState, id: u64, result: Result<AnalysisResult, AppError>) {
    let mut state = lock(shared);
    let output_config = state.config.output.clone();
    let track_config = state.config.tracks.clone();
    let audio_config = state.config.audio.clone();
    let encoder = state.config.encoder;

    // The job may have been removed while analysis was running
    let Some(job) = state.queue.job_by_id_mut(id) else {
        return;
    };

    match result {
        Ok(analysis) => {
            let is_av1 = is_av1_codec(&analysis.metadata.codec_name);
            let hdr_type = analysis.metadata.hdr_type;
            let dv_profile = analysis.metadata.dv_profile;
            job.metadata = Some(analysis.metadata);
            job.audio_tracks = analysis.audio_tracks;
            job.subtitle_tracks = analysis.subtitle_tracks;
            job.remux_only = is_av1;
            auto_select_tracks(job, &track_config, &audio_config);
            job.generate_output_path(&output_config);
            // Hardware encoders cannot write the DV RPU, so those jobs are
            // resolved to HDR10 (mirrors `App::maybe_open_dv_dialog`)
            if !is_av1 && hdr_type == HdrType::DolbyVision {
                job.dv_mode = Some(if encoder == Encoder::SvtAv1 {
                    DvMode::recommended_for(dv_profile)
                } else {
                    DvMode::ToHdr10
                });
            }
            job.status = JobStatus::AwaitingConfig;
            info!("Analyzed {}", job.path.display());
            make_output_paths_unique(&mut state.queue.state.jobs);
        }
        Err(e) => {
            job.status = JobStatus::Error {
                message: e.to_string(),
            };
            state.queue.state.error_count += 1;
        }
    }
}

/// Start a new encode session if idle, not paused, and jobs are ready.
fn maybe_start_session(shared: &SharedState, worker_tx: &Sender<WorkerMessage>) {
    let mut state = lock(shared);
    if state.encoding_active || state.paused {
        return;
    }

    let output_config = state.config.output.clone();
    let audio_config = state.config.audio.clone();
    let mut job_ids: Vec<u64> = Vec::new();
    let mut worker_jobs: Vec<WorkerJob> = Vec::new();
    for (id, job) in state.queue.jobs_with_ids() {
        if !matches!(job.status, JobStatus::Ready) {
            continue;
        }
        let Some(metadata) = job.metadata.clone() else {
            continue;
        };
        let output = job.output_path.clone().unwrap_or_else(|| {
            let stem = job.path.file_stem().unwrap_or_default().to_string_lossy();
            let parent = job.path.parent().unwrap_or(std::path::Path::new("."));
            parent.join(format!(
                "{}{}.{}",
                stem, output_config.suffix, output_config.container
            ))
        });
        let selected_subs = crate::tracks::selected_subtitles(
            &job.subtitle_tracks,
            &job.track_selection.subtitle_indices,
        );
        worker_jobs.push(WorkerJob {
            index: worker_jobs.len(),
            subtitle_codecs: crate::tracks::subtitle_codecs_for(&output, &selected_subs),
            input: job.path.clone(),
            output,
            metadata,
            tracks: job
                .track_selection
                .resolve(&job.audio_tracks, &audio_config),
            dv_mode: job.dv_mode.unwrap_or_default(),
            remux_only: job.remux_only,
        });
        job_ids.push(id);
    }

    if worker_jobs.is_empty() {
        return;
    }
    info!("Starting encode session with {} job(s)", worker_jobs.len());

    for &id in &job_ids {
        if let Some(job) = state.queue.job_by_id_mut(id) {
            job.status = JobStatus::Pending;
        }
    }

    // Progress and ETA are reported for the session that is actually running.
    // Carrying totals or a start time across sessions would leave the dashboard
    // measuring against jobs that finished hours ago, with all the idle time in
    // between counted as encoding time.
    state.queue.state.total_jobs_to_encode = worker_jobs.len();
    state.queue.state.encoding_progress_done = 0;
    state.queue.state.start_time = Some(Instant::now());
    state.queue.state.end_time = None;

    let cancel_flag = Arc::new(AtomicBool::new(false));
    state.session = Some(EncodeSession {
        job_ids,
        cancel_flag: cancel_flag.clone(),
    });
    state.encoding_active = true;

    let config = state.config.clone();
    let tx = worker_tx.clone();
    thread::spawn(move || {
        // `run_worker` already contains a panic per job, so this is the
        // last-resort net for a panic outside one. It matters because the
        // channel cannot signal it: this daemon keeps its own sender alive, so
        // a dead worker never disconnects, it just stops talking — and the
        // session would stay open forever, blocking every later one.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_worker(worker_jobs, &config, &cancel_flag, &tx);
        }))
        .is_err()
        {
            warn!("Encode worker panicked; ending the session");
            let _ = tx.send(WorkerMessage::Cancelled);
        }
    });
}

/// Apply one worker message: mirrors the TUI's `process_progress_messages`,
/// but translates the session-local index to a stable job id first.
fn apply_worker_message(shared: &SharedState, msg: WorkerMessage) {
    let mut state = lock(shared);
    let Some(session_ids) = state.session.as_ref().map(|s| s.job_ids.clone()) else {
        return; // Late message from an already-finished session
    };
    let id_for = |idx: usize| session_ids.get(idx).copied();

    match msg {
        WorkerMessage::Progress(idx, progress) => {
            if let Some(id) = id_for(idx) {
                if let Some(index) = state.queue.index_of(id) {
                    state.queue.state.current_job_index = index;
                }
                if let Some(job) = state.queue.job_by_id_mut(id) {
                    job.status = JobStatus::Encoding { progress };
                }
            }
        }
        WorkerMessage::Verifying(idx) => {
            if let Some(job) = id_for(idx).and_then(|id| state.queue.job_by_id_mut(id)) {
                job.status = JobStatus::Verifying;
            }
        }
        WorkerMessage::Done(idx) => {
            finish_job(&mut state, id_for(idx), JobStatus::Done);
        }
        WorkerMessage::DoneWithVmaf(idx, score) => {
            finish_job(&mut state, id_for(idx), JobStatus::DoneWithVmaf { score });
        }
        WorkerMessage::DoneVmafFailed(idx, reason) => {
            finish_job(
                &mut state,
                id_for(idx),
                JobStatus::DoneVmafFailed { reason },
            );
        }
        WorkerMessage::QualityWarning(idx, vmaf, threshold) => {
            finish_job(
                &mut state,
                id_for(idx),
                JobStatus::QualityWarning { vmaf, threshold },
            );
        }
        WorkerMessage::Error(idx, message) => {
            if let Some(job) = id_for(idx).and_then(|id| state.queue.job_by_id_mut(id)) {
                job.status = JobStatus::Error { message };
                state.queue.state.error_count += 1;
                state.queue.state.encoding_progress_done += 1;
            }
        }
        WorkerMessage::SourceDeleted(idx) => {
            if let Some(job) = id_for(idx).and_then(|id| state.queue.job_by_id_mut(id)) {
                job.source_deleted = true;
            }
        }
        WorkerMessage::SourceKeptLowVmaf(idx, vmaf) => {
            if let Some(job) = id_for(idx).and_then(|id| state.queue.job_by_id_mut(id)) {
                job.source_kept_vmaf = Some(vmaf);
            }
        }
        WorkerMessage::Cancelled => {
            for &id in &session_ids {
                if let Some(job) = state.queue.job_by_id_mut(id)
                    && !is_terminal(&job.status)
                {
                    job.status = JobStatus::Skipped {
                        reason: "Cancelled".to_string(),
                    };
                    state.queue.state.skipped_count += 1;
                    // A cancelled job is done as far as the session goes, so
                    // progress still reaches 100% instead of stalling short.
                    state.queue.state.encoding_progress_done += 1;
                }
            }
        }
    }

    // Session over when every job in it reached a terminal state
    let all_done = session_ids.iter().all(|&id| {
        state
            .queue
            .job_by_id(id)
            .is_none_or(|job| is_terminal(&job.status))
    });
    if all_done {
        state.encoding_active = false;
        state.session = None;
        state.queue.state.end_time = Some(Instant::now());
        info!("Encode session finished");
    }
}

/// Mark a session job as successfully finished and record the output size.
fn finish_job(state: &mut DaemonState, id: Option<u64>, status: JobStatus) {
    let Some(job) = id.and_then(|id| state.queue.job_by_id_mut(id)) else {
        return;
    };
    job.status = status;
    if let Some(ref output_path) = job.output_path {
        job.output_size = std::fs::metadata(output_path).ok().map(|m| m.len());
    }
    state.queue.state.converted_count += 1;
    state.queue.state.encoding_progress_done += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_paths_reserves_each_source_once() {
        let dir = std::env::temp_dir().join(format!("av1c_daemon_add_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("movie.mkv");
        std::fs::write(&path, b"not a real video").unwrap();

        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        let (tx, _rx) = mpsc::channel();
        assert_eq!(
            add_paths(
                &shared,
                &tx,
                vec![path.clone(), dir.join(".").join("movie.mkv")]
            ),
            (1, 1)
        );
        assert_eq!(add_paths(&shared, &tx, vec![path]), (0, 1));
        assert_eq!(lock(&shared).queue.state.jobs.len(), 1);

        let _ = std::fs::remove_dir_all(dir);
    }
}
