pub mod api;
pub mod lifecycle;
pub mod server;
pub mod state;

use crate::analyzer::{self, AnalysisResult, DvMode, HdrType, is_av1_codec};
use crate::config::{AppConfig, AudioMode, Encoder};
use crate::error::AppError;
use crate::i18n::{Msg, t};
use crate::queue::{
    EncodingJob, JobStatus, WorkerJob, WorkerMessage, auto_select_tracks, run_worker,
};
use crate::utils::DependencyStatus;
use state::{Command, DaemonState, EncodeSession, SharedState, is_terminal, lock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// How often the orchestrator wakes up when no commands arrive.
const TICK: Duration = Duration::from_millis(250);
/// How long shutdown waits for the worker to acknowledge cancellation.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

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

    let lang = config.language;
    let listen = format!("{}:{}", config.daemon.bind_address, config.daemon.port);

    // Anyone who can reach the port can queue encodes, rewrite the settings and
    // browse the filesystem, so say so plainly when that is more than this host.
    if config.daemon.binds_publicly() && config.daemon.auth_token.is_empty() {
        println!("{}", t(lang, Msg::DaemonPublicNoToken));
        warn!("Daemon is reachable from the network with no auth_token set");
    }

    let shared: SharedState = Arc::new(Mutex::new(DaemonState::new(config)));
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (analysis_tx, analysis_rx) = mpsc::channel::<(u64, Result<AnalysisResult, AppError>)>();
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();

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

    let server_handle = {
        let shared = shared.clone();
        let cmd_tx = cmd_tx.clone();
        let shutdown = shutdown.clone();
        let listen = listen.clone();
        let server = server::bind(&listen)?;
        println!("{} http://{listen}", t(lang, Msg::DaemonListening));
        info!("Web UI listening on http://{listen}");
        thread::spawn(move || server::serve(&server, &shared, &cmd_tx, &shutdown))
    };

    // Main orchestrator loop: sole owner of all receivers, sole mutator of jobs.
    while !shutdown.load(Ordering::SeqCst) {
        match cmd_rx.recv_timeout(TICK) {
            Ok(cmd) => handle_command(&shared, &analysis_tx, cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(cmd) = cmd_rx.try_recv() {
            handle_command(&shared, &analysis_tx, cmd);
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

    let _ = server_handle.join();
    Ok(())
}

fn handle_command(
    shared: &SharedState,
    analysis_tx: &Sender<(u64, Result<AnalysisResult, AppError>)>,
    cmd: Command,
) {
    match cmd {
        Command::AddPaths(paths) => add_paths(shared, analysis_tx, paths),
        Command::RemoveJob(id) => {
            let mut state = lock(shared);
            if !state.in_active_session(id) {
                state.queue.remove(id);
            }
        }
        Command::SetPaused(paused) => {
            lock(shared).paused = paused;
        }
        Command::CancelEncoding => {
            let state = lock(shared);
            if let Some(session) = state.session.as_ref().filter(|_| state.encoding_active) {
                session.cancel_flag.store(true, Ordering::Relaxed);
            }
        }
        Command::UpdateConfig(config) => {
            // Takes effect for subsequent analysis/sessions; a running
            // worker already cloned its config.
            lock(shared).config = *config;
        }
        Command::SetTracks(id, selection) => {
            let mut state = lock(shared);
            // The job may have started encoding between the handler's check
            // and this point, and its tracks are already baked into the
            // running FFmpeg command by then.
            if state.in_active_session(id) {
                return;
            }
            if let Some(job) = state.queue.job_by_id_mut(id)
                && matches!(job.status, JobStatus::Ready | JobStatus::AwaitingConfig)
            {
                job.track_selection = selection;
            }
        }
        Command::ClearFinished => {
            let mut state = lock(shared);
            let finished: Vec<u64> = state
                .queue
                .jobs_with_ids()
                .filter(|(_, job)| is_terminal(&job.status))
                .map(|(id, _)| id)
                .collect();
            for id in finished {
                state.queue.remove(id);
            }
        }
    }
}

/// Queue new files and spawn a batch analysis thread for them.
fn add_paths(
    shared: &SharedState,
    analysis_tx: &Sender<(u64, Result<AnalysisResult, AppError>)>,
    paths: Vec<std::path::PathBuf>,
) {
    let mut to_analyze: Vec<(u64, String)> = Vec::new();
    {
        let mut state = lock(shared);
        // Paths already queued and not yet finished are skipped
        let existing: Vec<std::path::PathBuf> = state
            .queue
            .jobs_with_ids()
            .filter(|(_, job)| !is_terminal(&job.status))
            .map(|(_, job)| job.path.canonicalize().unwrap_or_else(|_| job.path.clone()))
            .collect();

        for path in paths {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if existing.contains(&canonical) {
                continue;
            }
            let mut job = EncodingJob::new(path);
            if let Some(p) = job.path.to_str() {
                let p = p.to_string();
                job.status = JobStatus::Analyzing;
                let id = state.queue.push(job);
                to_analyze.push((id, p));
            } else {
                job.status = JobStatus::Error {
                    message: "File path contains non-UTF-8 characters".to_string(),
                };
                state.queue.state.error_count += 1;
                state.queue.push(job);
            }
        }
    }

    if to_analyze.is_empty() {
        return;
    }
    let tx = analysis_tx.clone();
    thread::spawn(move || {
        for (id, path) in to_analyze {
            let result = analyzer::analyze(&path);
            if tx.send((id, result)).is_err() {
                break;
            }
        }
    });
}

/// Apply one finished analysis: mirror the TUI's `apply_analysis_results`,
/// then resolve the Dolby Vision mode non-interactively and mark the job
/// ready to encode.
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
            job.status = JobStatus::Ready;
            info!("Analyzed {}", job.path.display());
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
        let selected_subs: Vec<crate::tracks::SubtitleTrack> = job
            .subtitle_tracks
            .iter()
            .filter(|t| job.track_selection.subtitle_indices.contains(&t.index))
            .cloned()
            .collect();
        worker_jobs.push(WorkerJob {
            index: worker_jobs.len(),
            subtitle_codec: crate::tracks::subtitle_codec_for(&output, &selected_subs),
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
        run_worker(worker_jobs, &config, &cancel_flag, &tx);
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
