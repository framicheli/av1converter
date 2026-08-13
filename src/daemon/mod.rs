pub mod api;
pub mod lifecycle;
pub mod server;
pub mod state;

use crate::analyzer::{self, AnalysisResult, HdrType, is_av1_codec};
use crate::config::{AppConfig, AudioMode};
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
/// Number of HTTP worker threads, so a slow scan or listing does not stall the
/// dashboard poll behind it.
const SERVER_THREADS: usize = 4;

/// Run the headless daemon: web server + encoding orchestrator.
/// Blocks until SIGINT/SIGTERM.
#[allow(clippy::too_many_lines)]
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

    // Plain HTTP: the token and media paths travel unencrypted.
    if config.daemon.binds_publicly() {
        warn!("Daemon is network-facing over plain HTTP; use HTTPS termination or a trusted LAN");
    }

    let queue_file = lifecycle::queue_file();
    let (state, reprobe) = restore_state(config, &queue_file);
    let shared: SharedState = Arc::new(Mutex::new(state));
    let (analysis_tx, analysis_rx) = mpsc::channel::<(u64, Result<AnalysisResult, AppError>)>();
    let (probe_tx, probe_rx) = mpsc::channel::<(u64, String)>();
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();

    // One long-lived prober rather than a thread per add request, so a client
    // does not get to decide how many ffprobe children run at once. A panic is
    // caught, blamed on the file that caused it, and the next one picked up.
    let shutdown = Arc::new(AtomicBool::new(false));
    let analysis_handle = {
        let analysis_tx = analysis_tx.clone();
        let shutdown = shutdown.clone();
        thread::spawn(move || {
            for (id, path) in probe_rx {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    analyzer::analyze(&path, &shutdown)
                }))
                .unwrap_or_else(|_| {
                    Err(AppError::Analysis(format!("Analysis panicked on {path}")))
                });
                if analysis_tx.send((id, result)).is_err() {
                    break;
                }
            }
        })
    };

    // Now that the prober is listening, hand back the reloaded files that
    // never got analyzed.
    for request in reprobe {
        if probe_tx.send(request).is_err() {
            warn!("Analysis is not running; reloaded files will stay unanalyzed");
            break;
        }
    }

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
    // The bare address, not `url()`: stdout is the daemon log file in
    // background mode, and the token stays out of it. The tokenised URL is
    // printed to the terminal by whoever started us.
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
    let mut last_saved = Vec::new();
    let mut last_save_warning = None;
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
        persist_queue(
            &shared,
            &queue_file,
            &mut last_saved,
            &mut last_save_warning,
        );
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

    // Cancellation moves every unfinished job to a terminal state, and is
    // saved so the next launch does not resume them as interrupted work.
    persist_queue(
        &shared,
        &queue_file,
        &mut last_saved,
        &mut last_save_warning,
    );

    for handle in server_handles {
        let _ = handle.join();
    }
    // The analyzer owns ffprobe children; closing its input and joining it
    // leaves none behind.
    drop(probe_tx);
    let _ = analysis_handle.join();
    Ok(())
}

/// Build the starting state from the queue this daemon last wrote. Returns the
/// state to run with, and the reloaded files that were never analyzed, for the
/// caller to hand back to the prober.
fn restore_state(
    config: AppConfig,
    queue_file: &std::path::Path,
) -> (DaemonState, Vec<(u64, String)>) {
    let mut restored = crate::queue::state::load(queue_file);
    let browse_root = config.daemon.browse_root.clone();
    for job in &mut restored.state.jobs {
        let source = api::confined_path(&job.path, &browse_root);
        let output = job.output_path.as_ref().and_then(|output| {
            let parent = api::confined_path(output.parent()?, &browse_root)?;
            Some(parent.join(output.file_name()?))
        });
        if source.is_none() || (job.output_path.is_some() && output.is_none()) {
            // The queue API includes paths, so completed history is dropped
            // too when a newly tightened root excludes it.
            job.path = std::path::PathBuf::from("<outside browse_root>");
            job.output_path = None;
            if !is_terminal(&job.status) {
                job.status = JobStatus::Error {
                    message: "Saved job is outside the configured browse root".to_string(),
                };
                restored.state.error_count += 1;
            }
        } else if let Some(source) = source
            && !browse_root.is_empty()
        {
            // The resolved paths that passed confinement, not the spellings.
            job.path = source;
            if job.output_path.is_some() {
                job.output_path = output;
            }
        }
    }
    crate::queue::state::resume(&mut restored);
    let restored_jobs = restored.state.jobs.len();

    let reprobe: Vec<(u64, String)> = crate::queue::state::needs_analysis(&restored)
        .into_iter()
        .filter_map(|(index, path)| restored.ids.get(index).map(|&id| (id, path)))
        .collect();

    let mut state = DaemonState::new(config);
    state.queue = state::DaemonQueue::from_persisted(restored);
    for (id, _) in &reprobe {
        if let Some(job) = state.queue.job_by_id_mut(*id) {
            job.status = JobStatus::Analyzing;
        }
    }
    if restored_jobs > 0 {
        info!(
            "Reloaded {restored_jobs} job(s) from {} ({} to re-analyze)",
            queue_file.display(),
            reprobe.len()
        );
    }
    (state, reprobe)
}

/// Write the queue out if its serialized form changed since the last write.
/// The only save call site: handlers and worker messages mutate the queue
/// behind the mutex without saving. `JobStatus::Encoding` does not persist its
/// percentage, so a running encode does not churn the file.
///
// ponytail: re-serializes the queue once per tick to compare. That is O(jobs)
// four times a second; if a very large queue ever makes it show up, set a
// dirty flag in `DaemonQueue`'s mutators instead.
fn persist_queue(
    shared: &SharedState,
    path: &std::path::Path,
    last_saved: &mut Vec<u8>,
    last_warning: &mut Option<Instant>,
) {
    let json = {
        let state = lock(shared);
        let snapshot = state.queue.as_persistable();
        match serde_json::to_vec_pretty(&snapshot) {
            Ok(json) => json,
            Err(_) => return,
        }
    };
    if json == *last_saved {
        return;
    }
    match crate::queue::state::save_serialized(path, &json) {
        Ok(()) => {
            *last_saved = json;
            *last_warning = None;
        }
        // Retried every tick, but logged only once per outage.
        Err(e) => {
            if last_warning.is_none_or(|at| at.elapsed() >= Duration::from_mins(1)) {
                warn!("Could not save the queue to {}: {e}", path.display());
                *last_warning = Some(Instant::now());
            }
        }
    }
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

    // A dead prober is reported on the jobs themselves, which would otherwise
    // sit in the non-terminal `Analyzing`, blocking re-adds of the same file.
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
/// then resolve the Dolby Vision mode non-interactively and wait for the `WebUI`
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
            job.source_size = Some(analysis.source_identity.size_bytes());
            job.source_identity = Some(analysis.source_identity);
            job.metadata = Some(analysis.metadata);
            job.audio_tracks = analysis.audio_tracks;
            job.subtitle_tracks = analysis.subtitle_tracks;
            job.remux_only = is_av1;
            auto_select_tracks(job, &track_config, &audio_config);
            job.generate_output_path(&output_config);
            // Mirrors `App::maybe_open_dv_dialog`, shared with the web API.
            if !is_av1 && hdr_type == HdrType::DolbyVision {
                job.dv_mode = Some(api::resolved_dv_mode(encoder, dv_profile));
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

/// Start a new encode session if idle and jobs are ready.
fn maybe_start_session(shared: &SharedState, worker_tx: &Sender<WorkerMessage>) {
    let mut state = lock(shared);
    if state.encoding_active {
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
        let Some(source_identity) = job.source_identity.clone() else {
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
            source_identity,
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

    // Totals and start time are per-session, never carried across one, so
    // progress and ETA measure only the run in flight.
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
        // `run_worker` catches a panic per job; this covers one outside any
        // job. The channel cannot report it — this daemon holds its own sender
        // alive, so a dead worker stops talking without ever disconnecting.
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
                    // A cancelled job counts as done for the session.
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

    #[test]
    fn restored_jobs_outside_the_new_root_are_never_resumed() {
        let base = std::env::temp_dir().join(format!("av1c_restore_root_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let source = outside.join("movie.mkv");
        let output = outside.join("movie_av1.mkv");
        let partial = outside.join("movie_av1.part.123_4.mkv");
        std::fs::write(&source, b"source").unwrap();
        std::fs::write(&partial, b"partial").unwrap();

        let mut queue = crate::queue::QueueState::new();
        let mut job = EncodingJob::new(source);
        job.output_path = Some(output);
        job.status = JobStatus::Encoding { progress: 50.0 };
        queue.jobs.push(job);
        let queue_file = base.join("queue.json");
        crate::queue::state::save(
            &queue_file,
            &crate::queue::QueueRef {
                state: &queue,
                ids: &[1],
                next_id: 2,
            },
        )
        .unwrap();
        let config = AppConfig {
            daemon: crate::config::DaemonConfig {
                browse_root: root.to_string_lossy().into_owned(),
                ..crate::config::DaemonConfig::default()
            },
            ..AppConfig::default()
        };

        let (state, reprobe) = restore_state(config, &queue_file);
        assert!(matches!(
            state.queue.state.jobs[0].status,
            JobStatus::Error { .. }
        ));
        assert!(reprobe.is_empty());
        assert!(
            partial.exists(),
            "resume must not delete files outside browse_root"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn shutdown_cancellation_is_saved_as_terminal() {
        let dir =
            std::env::temp_dir().join(format!("av1c_shutdown_persist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let queue_file = dir.join("queue.json");

        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        {
            let mut state = lock(&shared);
            let id = state
                .queue
                .push(EncodingJob::new(std::path::PathBuf::from("/tmp/movie.mkv")));
            state.queue.job_by_id_mut(id).unwrap().status = JobStatus::Pending;
            state.session = Some(EncodeSession {
                job_ids: vec![id],
                cancel_flag: Arc::new(AtomicBool::new(true)),
            });
            state.encoding_active = true;
        }

        apply_worker_message(&shared, WorkerMessage::Cancelled);
        let mut last_saved = Vec::new();
        let mut last_warning = None;
        persist_queue(&shared, &queue_file, &mut last_saved, &mut last_warning);

        let saved = crate::queue::state::load(&queue_file);
        assert!(matches!(
            saved.state.jobs[0].status,
            JobStatus::Skipped { .. }
        ));
        let _ = std::fs::remove_dir_all(dir);
    }
}
