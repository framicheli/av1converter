pub mod api;
pub mod lifecycle;
pub mod server;
pub mod service;
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
use std::sync::mpsc::{self, Receiver, Sender};
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
    // One channel for every disc run: the handlers start runs, this loop
    // applies what they report.
    let (disc_tx, disc_rx) = mpsc::channel::<crate::disc::worker::DiscEvent>();

    // One long-lived prober rather than a thread per add request, so a client
    // does not get to decide how many ffprobe children run at once. A panic is
    // caught, blamed on the file that caused it, and the next one picked up.
    let shutdown = Arc::new(AtomicBool::new(false));
    lock(&shared).shutting_down = shutdown.clone();
    let analysis_handle = {
        let analysis_tx = analysis_tx.clone();
        let shutdown = shutdown.clone();
        let shared = shared.clone();
        thread::spawn(move || {
            for (id, path) in probe_rx {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let cancel = lock(&shared).analysis_cancel.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    analyzer::analyze(&path, &cancel)
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
                // Second signal: kill every tracked ffmpeg/makemkvcon child
                // and exit.
                crate::utils::child::kill_all();
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
            let disc_tx = disc_tx.clone();
            let shutdown = shutdown.clone();
            thread::spawn(move || server::serve(&server, &shared, &probe_tx, &disc_tx, &shutdown))
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

        while let Ok(event) = disc_rx.try_recv() {
            apply_disc_event(&shared, &probe_tx, event);
        }

        {
            let mut state = lock(&shared);
            crate::disc::staging::cleanup_finished(&mut state.queue.state.jobs);
        }
        maybe_start_session(&shared, &worker_tx);
        persist_queue(
            &shared,
            &queue_file,
            &mut last_saved,
            &mut last_save_warning,
        );
    }

    // HTTP stops accepting once `shutdown` is set. Join it before taking
    // worker handles so an in-flight rip is stored (and then cancelled)
    // rather than spawned after take().
    for handle in server_handles {
        let _ = handle.join();
    }

    lock(&shared).analysis_cancel.store(true, Ordering::Relaxed);

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

    // A rip is waited out the same way: makemkvcon is a child of this process
    // and exiting while it runs would leave it behind.
    if lock(&shared).disc.active {
        lock(&shared).disc.cancel();
        println!("{}", t(lang, Msg::DaemonShuttingDown));
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline && lock(&shared).disc.active {
            match disc_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(event) => apply_disc_event(&shared, &probe_tx, event),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
    if cancelling {
        println!("{}", t(lang, Msg::DaemonShuttingDown));
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline {
            match worker_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(msg) => {
                    let done = matches!(msg, WorkerMessage::Cancelled | WorkerMessage::Finished);
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

    if lock(&shared).encoding_active || lock(&shared).disc.active {
        warn!("shutdown grace elapsed; killing leftover child processes");
        crate::utils::child::kill_all();
    }
    let (encode_worker, disc_worker) = {
        let mut state = lock(&shared);
        (state.encode_worker.take(), state.disc_worker.take())
    };
    if let Some(handle) = encode_worker {
        let _ = handle.join();
    }
    if let Some(handle) = disc_worker {
        let _ = handle.join();
    }
    drain_shutdown_channels(&shared, &worker_rx, &disc_rx, &probe_tx);

    // Cancellation moves every unfinished job to a terminal state, and is
    // saved so the next launch does not resume them as interrupted work.
    persist_queue(
        &shared,
        &queue_file,
        &mut last_saved,
        &mut last_save_warning,
    );
    // The analyzer owns ffprobe children; closing its input and joining it
    // leaves none behind.
    drop(probe_tx);
    let _ = analysis_handle.join();
    Ok(())
}

/// Apply leftover worker/disc messages after join, then skip anything still
/// in flight so the saved queue does not come back as Ready/Encoding.
fn drain_shutdown_channels(
    shared: &SharedState,
    worker_rx: &Receiver<WorkerMessage>,
    disc_rx: &Receiver<crate::disc::worker::DiscEvent>,
    probe_tx: &Sender<(u64, String)>,
) {
    while let Ok(msg) = worker_rx.try_recv() {
        apply_worker_message(shared, msg);
    }
    while let Ok(event) = disc_rx.try_recv() {
        apply_disc_event(shared, probe_tx, event);
    }
    if lock(shared).encoding_active {
        apply_worker_message(shared, WorkerMessage::Cancelled);
    }
    if lock(shared).disc.active {
        apply_disc_event(shared, probe_tx, crate::disc::worker::DiscEvent::Cancelled);
    }
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
        // Staged rips live in the staging directory, outside any browse root.
        let source = if job.temporary {
            Some(job.path.clone())
        } else {
            api::confined_path(&job.path, &browse_root)
        };
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
    // A rip cut short by a kill has nothing left tracking its file.
    crate::disc::staging::sweep_orphans(
        &state.config,
        &state.queue.state.jobs,
        crate::disc::staging::ACTIVE_RIP_WINDOW,
    );
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

/// Apply one event from a disc run, and hand each extracted file straight to
/// the prober: the drive moves on to the next title while this one is probed.
fn apply_disc_event(
    shared: &SharedState,
    probe_tx: &Sender<(u64, String)>,
    event: crate::disc::worker::DiscEvent,
) {
    use crate::disc::worker::DiscEvent;

    let mut ready = None;
    {
        let mut state = lock(shared);
        let lang = state.config.language;
        match event {
            DiscEvent::TitlesFound(scan) => {
                state.disc.disc_type = scan.disc_type;
                // A scan that settles with zero titles reports as an error.
                if scan.titles.is_empty() {
                    state.disc.error = Some(t(lang, Msg::DiscNoTitles).to_string());
                }
                state.disc.titles = scan.titles;
                state.disc.settle();
            }
            DiscEvent::Ripping { index, progress } => {
                if let Some(id) = state.disc.job_ids.get(index).copied()
                    && let Some(job) = state.queue.job_by_id_mut(id)
                    && matches!(job.status, JobStatus::Ripping { .. })
                {
                    job.status = JobStatus::Ripping { progress };
                }
            }
            DiscEvent::TitleReady { index, path } => {
                if let Some(id) = state.disc.job_ids.get(index).copied()
                    && let Some(job) = state.queue.job_by_id_mut(id)
                    && matches!(job.status, JobStatus::Ripping { .. })
                {
                    job.source_size = std::fs::metadata(&path).ok().map(|m| m.len());
                    job.path = path;
                    job.status = JobStatus::Analyzing;
                    ready = job.path.to_str().map(|path| (id, path.to_string()));
                    state.analysis_cancel = Arc::new(AtomicBool::new(false));
                }
            }
            DiscEvent::Error { index, error } => {
                let message = error.message(lang);
                info!("Disc run stopped: {message}");
                let ids = state.disc.job_ids.clone();
                for (position, id) in ids.into_iter().enumerate() {
                    let failed = position == index;
                    let Some(job) = state.queue.job_by_id_mut(id) else {
                        continue;
                    };
                    if !matches!(job.status, JobStatus::Ripping { .. }) {
                        continue;
                    }
                    job.status = if failed {
                        JobStatus::Error {
                            message: message.clone(),
                        }
                    } else {
                        JobStatus::Skipped {
                            reason: "Cancelled".to_string(),
                        }
                    };
                    if failed {
                        state.queue.state.error_count += 1;
                    } else {
                        state.queue.state.skipped_count += 1;
                    }
                }
                state.disc.error = Some(message);
                state.disc.settle();
            }
            DiscEvent::Cancelled => {
                let ids = state.disc.job_ids.clone();
                for id in ids {
                    if let Some(job) = state.queue.job_by_id_mut(id)
                        && matches!(job.status, JobStatus::Ripping { .. })
                    {
                        job.status = JobStatus::Skipped {
                            reason: "Cancelled".to_string(),
                        };
                        state.queue.state.skipped_count += 1;
                    }
                }
                state.disc.settle();
            }
            DiscEvent::Finished => state.disc.settle(),
        }
    }

    if let Some((id, path)) = ready
        && probe_tx.send((id, path)).is_err()
    {
        let mut state = lock(shared);
        if let Some(job) = state.queue.job_by_id_mut(id) {
            job.status = JobStatus::Error {
                message: "Analysis is not running".to_string(),
            };
        }
        state.queue.state.error_count += 1;
    }
}

/// Write the queue out if its serialized form changed since the last write.
/// The only save call site: handlers and worker messages mutate the queue
/// behind the mutex without saving. `JobStatus::Encoding` does not persist its
/// percentage, so a running encode does not churn the file.
///
// Re-serializes the queue once per tick to compare: O(jobs) four times a
// second. A dirty flag in `DaemonQueue`'s mutators is the upgrade if a very
// large queue ever makes it show up.
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

    // Canonicalization touches the filesystem and runs with no lock held.
    // Jobs added by other requests in between are caught by the raw-path
    // recheck under the lock; two different spellings of one file added
    // concurrently can both queue.
    let queued_paths: Vec<std::path::PathBuf> = {
        let state = lock(shared);
        state
            .queue
            .jobs_with_ids()
            .filter(|(_, job)| !is_terminal(&job.status))
            .map(|(_, job)| job.path.clone())
            .collect()
    };
    let mut existing: std::collections::HashSet<std::path::PathBuf> = queued_paths
        .iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
        .collect();
    let canonical_paths: Vec<(std::path::PathBuf, std::path::PathBuf)> = paths
        .into_iter()
        .map(|path| (path.canonicalize().unwrap_or_else(|_| path.clone()), path))
        .collect();

    {
        let mut state = lock(shared);
        state.analysis_cancel = Arc::new(AtomicBool::new(false));
        state.queue.state.reset_session_if_finished();
        for (_, job) in state
            .queue
            .jobs_with_ids()
            .filter(|(_, job)| !is_terminal(&job.status))
        {
            if !queued_paths.contains(&job.path) {
                existing.insert(job.path.clone());
            }
        }

        // Paths already queued and not yet finished are skipped
        for (canonical, path) in canonical_paths {
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
    if !job.status.awaits_analysis() {
        return;
    }

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
        Err(e) if e.to_string().contains("Cancelled") => {
            job.status = JobStatus::Skipped {
                reason: "Cancelled".to_string(),
            };
            state.queue.state.skipped_count += 1;
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
        // A ripped file has no next-to-the-source fallback: that is the staging
        // directory.
        let output = match job.output_path.clone() {
            Some(output) => output,
            None if job.temporary => continue,
            None => {
                let stem = job.path.file_stem().unwrap_or_default().to_string_lossy();
                let parent = job.path.parent().unwrap_or(std::path::Path::new("."));
                parent.join(format!(
                    "{}{}.{}",
                    stem, output_config.suffix, output_config.container
                ))
            }
        };
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
        break;
    }

    if worker_jobs.is_empty() {
        return;
    }
    info!("Starting encode session with one job");

    let ready = state
        .queue
        .state
        .jobs
        .iter()
        .filter(|job| matches!(job.status, JobStatus::Ready))
        .count();
    state.queue.state.total_jobs_to_encode = state.queue.state.encoding_progress_done + ready;
    // The elapsed clock counts encoding time only; the idle gap since the
    // last session ended is shifted out of it.
    let now = Instant::now();
    match (state.queue.state.start_time, state.queue.state.end_time) {
        (Some(start), Some(end)) => {
            state.queue.state.start_time = Some(start + now.duration_since(end));
        }
        (None, _) => state.queue.state.start_time = Some(now),
        (Some(_), None) => {}
    }
    state.queue.state.end_time = None;
    if let Some(job) = job_ids
        .first()
        .and_then(|&id| state.queue.job_by_id_mut(id))
    {
        job.status = JobStatus::Encoding { progress: 0.0 };
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    state.session = Some(EncodeSession {
        job_ids,
        cancel_flag: cancel_flag.clone(),
    });
    state.encoding_active = true;

    let config = state.config.clone();
    let tx = worker_tx.clone();
    state.encode_worker = Some(thread::spawn(move || {
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
    }));
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
        WorkerMessage::Finished => {}
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
        // Set on every session end, not only a fully settled queue.
        state.queue.state.end_time = Some(Instant::now());
        info!("Encode job finished");
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
    fn a_finished_job_leaves_the_run_open_for_the_next_ready_job() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut active = EncodingJob::new(std::path::PathBuf::from("active.mkv"));
        active.status = JobStatus::Encoding { progress: 50.0 };
        let active_id = state.queue.push(active);
        let mut waiting = EncodingJob::new(std::path::PathBuf::from("waiting.mkv"));
        waiting.status = JobStatus::Ready;
        state.queue.push(waiting);
        state.queue.state.total_jobs_to_encode = 2;
        state.session = Some(EncodeSession {
            job_ids: vec![active_id],
            cancel_flag: Arc::new(AtomicBool::new(false)),
        });
        state.encoding_active = true;
        let shared = Arc::new(Mutex::new(state));

        apply_worker_message(&shared, WorkerMessage::Done(0));

        let state = lock(&shared);
        assert!(!state.encoding_active);
        // The pause marker is set; the next session start clears it and
        // subtracts the gap from the elapsed clock.
        assert!(state.queue.state.end_time.is_some());
        assert_eq!(state.queue.state.encoding_progress_done, 1);
        assert!(matches!(state.queue.state.jobs[1].status, JobStatus::Ready));
    }

    #[test]
    fn a_ripped_title_starts_with_a_fresh_analysis_token() {
        use crate::disc::worker::DiscEvent;

        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        let (probe_tx, probe_rx) = mpsc::channel();
        {
            let mut state = lock(&shared);
            let mut job = EncodingJob::new(std::path::PathBuf::from("Title 0"));
            job.status = JobStatus::Ripping { progress: 0.0 };
            job.temporary = true;
            let id = state.queue.push(job);
            state.disc.job_ids = vec![id];
            state.disc.active = true;
            state.analysis_cancel.store(true, Ordering::Relaxed);
        }

        apply_disc_event(
            &shared,
            &probe_tx,
            DiscEvent::TitleReady {
                index: 0,
                path: std::path::PathBuf::from("/staging/rip-a/DISC_t00.mkv"),
            },
        );

        assert!(probe_rx.try_recv().is_ok());
        assert!(!lock(&shared).analysis_cancel.load(Ordering::Relaxed));
    }

    /// An extracted title is repointed at its file and handed to the prober
    /// right away, while the drive carries on with the next one.
    #[test]
    fn a_ripped_title_goes_straight_to_the_prober() {
        use crate::disc::worker::DiscEvent;

        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        let (probe_tx, probe_rx) = mpsc::channel();
        let ids: Vec<u64> = {
            let mut state = lock(&shared);
            let ids = ["Title 0", "Title 1"]
                .iter()
                .map(|name| {
                    let mut job = EncodingJob::new(std::path::PathBuf::from(*name));
                    job.status = JobStatus::Ripping { progress: 0.0 };
                    job.temporary = true;
                    state.queue.push(job)
                })
                .collect();
            state.disc.job_ids = ids;
            state.disc.active = true;
            state.disc.job_ids.clone()
        };

        apply_disc_event(
            &shared,
            &probe_tx,
            DiscEvent::Ripping {
                index: 0,
                progress: 42.0,
            },
        );
        assert!(matches!(
            lock(&shared).queue.job_by_id(ids[0]).unwrap().status,
            JobStatus::Ripping { progress } if (progress - 42.0).abs() < f64::EPSILON
        ));

        apply_disc_event(
            &shared,
            &probe_tx,
            DiscEvent::TitleReady {
                index: 0,
                path: std::path::PathBuf::from("/staging/rip-a/DISC_t00.mkv"),
            },
        );
        assert_eq!(
            probe_rx.try_recv().unwrap(),
            (ids[0], "/staging/rip-a/DISC_t00.mkv".to_string())
        );
        assert!(matches!(
            lock(&shared).queue.job_by_id(ids[0]).unwrap().status,
            JobStatus::Analyzing
        ));
        apply_disc_event(
            &shared,
            &probe_tx,
            DiscEvent::Ripping {
                index: 0,
                progress: 99.0,
            },
        );
        assert!(
            matches!(
                lock(&shared).queue.job_by_id(ids[0]).unwrap().status,
                JobStatus::Analyzing
            ),
            "late rip progress must not overwrite a title that already extracted"
        );
        // The run is not over: the second title is still to come.
        assert!(lock(&shared).disc.active);

        apply_disc_event(
            &shared,
            &probe_tx,
            DiscEvent::Error {
                index: 1,
                error: crate::disc::DiscError::UnreadableDisc,
            },
        );
        let state = lock(&shared);
        assert!(matches!(
            state.queue.job_by_id(ids[1]).unwrap().status,
            JobStatus::Error { .. }
        ));
        assert!(!state.disc.active, "a failure ends the run");
        assert!(state.disc.error.is_some());
        assert_eq!(state.queue.state.error_count, 1);
        // A title that never became a file is not left waiting to be encoded
        // from one: only the extracted title is still in flight, at the prober.
        assert!(
            state.queue.state.jobs.iter().all(|job| !matches!(
                job.status,
                JobStatus::Ripping { .. } | JobStatus::Ready | JobStatus::Pending
            )),
            "a failed extraction left a job queued against a file that was never written"
        );
    }

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
    fn adding_after_finished_queue_starts_fresh_totals() {
        let dir = std::env::temp_dir().join(format!("av1c_daemon_fresh_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("new.mkv");
        std::fs::write(&path, b"not a real video").unwrap();

        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        {
            let mut state = lock(&shared);
            let mut old = EncodingJob::new(std::path::PathBuf::from("/tmp/old.mkv"));
            old.status = JobStatus::Error {
                message: "old failure".to_string(),
            };
            state.queue.push(old);
            state.queue.state.converted_count = 7;
            state.queue.state.skipped_count = 9;
            state.queue.state.error_count = 3;
        }
        let (tx, _rx) = mpsc::channel();

        assert_eq!(add_paths(&shared, &tx, vec![path]), (1, 0));
        let state = lock(&shared);
        assert_eq!(state.queue.state.converted_count, 0);
        assert_eq!(state.queue.state.skipped_count, 0);
        assert_eq!(state.queue.state.error_count, 0);

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

        let staged = outside.join("DISC_t00.mkv");
        std::fs::write(&staged, b"staged").unwrap();

        let mut queue = crate::queue::QueueState::new();
        let mut job = EncodingJob::new(source);
        job.output_path = Some(output);
        job.status = JobStatus::Encoding { progress: 50.0 };
        queue.jobs.push(job);
        let mut rip = EncodingJob::new(staged.clone());
        rip.status = JobStatus::Ready;
        rip.temporary = true;
        rip.metadata = Some(crate::analyzer::VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: crate::analyzer::HdrType::Sdr,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "hevc".to_string(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 1.0,
        });
        rip.source_identity = Some(crate::queue::SourceIdentity::from_metadata(
            &std::fs::metadata(&staged).unwrap(),
        ));
        queue.jobs.push(rip);
        let queue_file = base.join("queue.json");
        crate::queue::state::save(
            &queue_file,
            &crate::queue::QueueRef {
                state: &queue,
                ids: &[1, 2],
                next_id: 3,
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
        assert!(matches!(state.queue.state.jobs[1].status, JobStatus::Ready));
        assert_eq!(state.queue.state.jobs[1].path, staged);
        assert!(staged.exists());
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

    #[test]
    fn a_late_analysis_error_does_not_overwrite_a_failed_job() {
        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        let id = {
            let mut state = lock(&shared);
            let mut job = EncodingJob::new(std::path::PathBuf::from("gone.mkv"));
            job.status = JobStatus::Error {
                message: "probe failed".to_string(),
            };
            state.queue.push(job)
        };
        apply_analysis_result(&shared, id, Err(AppError::Analysis("late".to_string())));
        let state = lock(&shared);
        assert!(matches!(
            &state.queue.job_by_id(id).unwrap().status,
            JobStatus::Error { message } if message == "probe failed"
        ));
        assert_eq!(state.queue.state.error_count, 0);
    }

    #[test]
    fn draining_shutdown_skips_a_job_still_marked_encoding() {
        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        let id = {
            let mut state = lock(&shared);
            let mut job = EncodingJob::new(std::path::PathBuf::from("movie.mkv"));
            job.status = JobStatus::Encoding { progress: 40.0 };
            let id = state.queue.push(job);
            state.session = Some(EncodeSession {
                job_ids: vec![id],
                cancel_flag: Arc::new(AtomicBool::new(true)),
            });
            state.encoding_active = true;
            id
        };
        let (worker_tx, worker_rx) = mpsc::channel();
        drop(worker_tx);
        let (disc_tx, disc_rx) = mpsc::channel();
        drop(disc_tx);
        let (probe_tx, _probe_rx) = mpsc::channel();

        drain_shutdown_channels(&shared, &worker_rx, &disc_rx, &probe_tx);

        let state = lock(&shared);
        assert!(matches!(
            &state.queue.job_by_id(id).unwrap().status,
            JobStatus::Skipped { reason } if reason == "Cancelled"
        ));
        assert!(!state.encoding_active);
    }
}
