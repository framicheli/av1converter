use super::state::{SharedState, is_terminal, lock};
use crate::analyzer::{DvMode, HdrType};
use crate::config::{AppConfig, Encoder};
use crate::disc::worker::DiscEvent;
use crate::i18n::Msg;
pub use crate::queue::job::resolved_dv_mode;
use crate::queue::job::{apply_to_remaining, apply_track_config};
use crate::queue::{
    EncodingJob, JobStatus, collect_video_files_cancellable_result, collect_video_files_within,
    is_video_file, make_output_paths_unique,
};
use crate::tracks::TrackSelection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

/// Dashboard poll: overall daemon and encode-session status.
pub fn status(shared: &SharedState) -> Value {
    let state = lock(shared);
    let queue = &state.queue.state;

    // Current is the encoding index while it holds an encode or a verify, and
    // otherwise the first ripping or analyzing job in the queue.
    let current_index = Some(queue.current_job_index)
        .filter(|i| {
            queue.jobs.get(*i).is_some_and(|job| {
                matches!(
                    job.status,
                    JobStatus::Encoding { .. } | JobStatus::Verifying
                )
            })
        })
        .or_else(|| {
            queue.jobs.iter().position(|job| {
                matches!(job.status, JobStatus::Ripping { .. } | JobStatus::Analyzing)
            })
        });
    let current = current_index.and_then(|i| {
        queue.jobs.get(i).map(|job| {
            json!({
                "id": state.queue.ids().get(i),
                "filename": job.filename(),
                "status": status_json(&job.status),
            })
        })
    });

    let ready = queue
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Ready))
        .count();
    let pending = queue
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Pending))
        .count();
    let analyzing = queue
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Analyzing))
        .count();
    let awaiting_config = queue
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::AwaitingConfig))
        .count();
    let active = queue
        .jobs
        .iter()
        .filter(|j| !is_terminal(&j.status))
        .count();
    let (saved_bytes, saved_human) = queue.total_space_saved();

    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "encoder": state.config.encoder.display_name(),
        "language": state.config.language,
        "deps": {
            "ffmpeg": state.deps.ffmpeg,
            "vmaf": state.deps.vmaf,
            "opus": state.deps.opus,
            "encoder": state.deps.encoder,
        },
        "unreadable_queue": state.unreadable_queue.as_ref().map(|p| p.display().to_string()),
        "vmaf_enabled": state.config.quality.vmaf_enabled,
        "vmaf_threshold": state.config.quality.vmaf_threshold,
        "encoding_active": state.encoding_active,
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "overall_progress": queue.overall_progress(),
        "eta_secs": queue.estimated_time_remaining().map(|d| d.as_secs()),
        "elapsed_secs": queue.elapsed_time().map(|d| d.as_secs()),
        "counts": {
            "total": queue.jobs.len(),
            "active": active,
            "pending": pending,
            "analyzing": analyzing,
            "awaiting_config": awaiting_config,
            "ready": ready,
            "converted": queue.converted_count,
            // `skipped_count` includes the cancelled jobs.
            "skipped": queue.skipped_count.saturating_sub(queue.cancelled_count),
            "cancelled": queue.cancelled_count,
            "errors": queue.error_count,
        },
        "total_space_saved": { "bytes": saved_bytes, "human": saved_human },
        "current": current,
        // Disc state rides along here; the dashboard already polls this.
        "disc": disc_status(&state.disc),
    })
}

fn disc_status(disc: &super::state::DiscSession) -> Value {
    let titles: Vec<Value> = disc
        .titles
        .iter()
        .map(|title| {
            json!({
                "id": title.id,
                "name": title.name,
                "duration_secs": title.duration.as_secs(),
                "duration": crate::utils::format_duration(title.duration),
                "size_bytes": title.size_bytes,
                "size": crate::utils::format_file_size(title.size_bytes),
                "chapters": title.chapters,
                "tracks": title.tracks,
            })
        })
        .collect();

    // A scan names the source it came from: a drive id, or the folder path.
    let (drive, folder) = match disc.scanned_source.as_ref() {
        Some(crate::disc::DiscSource::Drive(drive)) => (json!(drive.id), Value::Null),
        Some(crate::disc::DiscSource::Folder(path)) => (Value::Null, json!(path.to_string_lossy())),
        None => (Value::Null, Value::Null),
    };

    // A drive listing holds the drive but is not a run the client can act on.
    json!({
        "active": disc.active && !disc.listing,
        "scanning": disc.scanning,
        "drive": drive,
        "folder": folder,
        "disc_type": disc.disc_type,
        "titles": titles,
        "error": disc.error,
    })
}

/// Full job list for the queue tab.
pub fn queue(shared: &SharedState) -> Value {
    let state = lock(shared);
    let jobs: Vec<Value> = state
        .queue
        .jobs_with_ids()
        .enumerate()
        .map(|(index, (id, job))| {
            let saved_percent = job.size_reduction().map(|(_, percent)| percent);
            let can_move_up = state.queue.state.can_move_ready_up(index);
            let vmaf = match job.status {
                JobStatus::DoneWithVmaf { score } => Some(score),
                JobStatus::QualityWarning { vmaf, .. } => Some(vmaf),
                _ => None,
            };
            json!({
                "id": id,
                "filename": job.filename(),
                "path": job.path.to_string_lossy(),
                "output_path": job.output_path.as_ref().map(|p| p.to_string_lossy()),
                "status": status_json(&job.status),
                // Null until the probe has run; the page shows a placeholder.
                "resolution": job.metadata.as_ref().map(|_| job.resolution_string()),
                "hdr": job.metadata.as_ref().map(|_| job.hdr_string()),
                "remux_only": job.remux_only,
                "source_size": job.source_size,
                "output_size": job.output_size,
                "saved_percent": saved_percent,
                "source_deleted": job.source_deleted,
                // A ripped file whose staging copy is deleted with the job.
                "temporary": job.temporary,
                "tracks_editable": tracks_editable(&state, id, job),
                "can_move_up": can_move_up,
                "crf": job.crf,
                "source_kept_vmaf": job.source_kept_vmaf,
                "source_kept_reason": job
                    .source_kept_reason
                    .map(|reason| crate::i18n::t(state.config.language, reason.msg())),
                "output_name": job
                    .output_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy()),
                "quality": vmaf
                    .filter(|score| score.is_finite())
                    .map(|score| crate::i18n::quality_description(state.config.language, score)),
            })
        })
        .collect();
    json!({ "jobs": jobs })
}

/// Whether a job's tracks can still be changed. False once its encode is under
/// way: the selection is baked into the running `FFmpeg` command.
fn tracks_editable(state: &super::state::DaemonState, id: u64, job: &EncodingJob) -> bool {
    !state.in_active_session(id)
        && matches!(job.status, JobStatus::Ready | JobStatus::AwaitingConfig)
}

const DV_KEEP: &str = "keep";
const DV_HDR10: &str = "hdr10";

fn dv_mode_name(mode: DvMode) -> &'static str {
    match mode {
        DvMode::KeepDolbyVision => DV_KEEP,
        DvMode::ToHdr10 => DV_HDR10,
    }
}

/// One job's audio and subtitle tracks, with the current per-track choices.
#[allow(clippy::too_many_lines)]
pub fn job_tracks(shared: &SharedState, id_param: &str) -> (u16, Value) {
    let Ok(id) = id_param.parse::<u64>() else {
        return (400, json!({"error": "missing or invalid 'id'"}));
    };
    let state = lock(shared);
    let audio_config = state.config.audio.clone();
    let encoder = state.config.encoder;
    let Some(job) = state.queue.job_by_id(id) else {
        return (404, json!({"error": "unknown job id"}));
    };
    let remaining = state
        .queue
        .jobs_with_ids()
        .filter(|(other_id, other)| {
            *other_id != id && matches!(other.status, JobStatus::AwaitingConfig)
        })
        .count();

    // The output path with remux off and on: a remux keeps the source container.
    let outputs = [false, true].map(|remux_only| {
        let mut probe = job.clone();
        probe.remux_only = remux_only;
        probe.generate_output_path(&state.config.output);
        probe.output_path.unwrap_or_default()
    });
    let output = &outputs[usize::from(job.remux_only)];

    // Resolved so the row shows the bitrate the encoder is actually asked for,
    // including already-Opus tracks, which are left alone.
    let plan = job
        .track_selection
        .resolve_for(&job.audio_tracks, &audio_config, output);
    let audio: Vec<Value> = job
        .audio_tracks
        .iter()
        .map(|track| {
            // The Opus bitrate the container forces on this track when copied.
            let copied = TrackSelection {
                audio_indices: vec![track.index],
                ..TrackSelection::default()
            };
            let forced_kbps = |output: &Path| {
                copied
                    .resolve_for(&job.audio_tracks, &audio_config, output)
                    .audio
                    .first()
                    .and_then(|p| p.opus_kbps)
            };
            json!({
                "index": track.index,
                "name": track.display_name(),
                "codec": track.codec,
                "channels": track.channels,
                "bitrate": track.bitrate_string(),
                "sample_rate": track.sample_rate_string(),
                "selected": job.track_selection.audio_indices.contains(&track.index),
                "opus": job.track_selection.is_opus(track.index),
                "opus_kbps": plan
                    .audio
                    .iter()
                    .find(|p| p.source_index == track.index)
                    .and_then(|p| p.opus_kbps),
                "container_opus_kbps": {
                    "encode": forced_kbps(&outputs[0]),
                    "remux": forced_kbps(&outputs[1]),
                },
            })
        })
        .collect();

    let subtitles: Vec<Value> = job
        .subtitle_tracks
        .iter()
        .map(|track| {
            let dropped = |output: &Path| {
                crate::tracks::subtitle_codecs_for(output, std::slice::from_ref(track))
                    .first()
                    .is_some_and(Option::is_none)
            };
            json!({
                "index": track.index,
                "name": track.display_name(),
                "selected": job.track_selection.subtitle_indices.contains(&track.index),
                "container_drops": {
                    "encode": dropped(&outputs[0]),
                    "remux": dropped(&outputs[1]),
                },
            })
        })
        .collect();

    // A DV choice only exists for a Dolby Vision source. `can_keep` carries the
    // encoder constraint to the UI, which refuses the option without it.
    let dv = job
        .metadata
        .as_ref()
        .filter(|meta| meta.hdr_type == HdrType::DolbyVision)
        .map(|meta| {
            // A stored mode applies only on SVT-AV1; other encoders write HDR10.
            let effective = job
                .dv_mode
                .filter(|_| encoder == Encoder::SvtAv1)
                .unwrap_or_else(|| resolved_dv_mode(encoder, meta.dv_profile));
            json!({
                "profile": meta.dv_profile,
                "mode": dv_mode_name(effective),
                "recommended": dv_mode_name(resolved_dv_mode(encoder, meta.dv_profile)),
                "can_keep": encoder == Encoder::SvtAv1,
            })
        });

    (
        200,
        json!({
            "id": id,
            "filename": job.filename(),
            "editable": tracks_editable(&state, id, job),
            "audio": audio,
            "subtitles": subtitles,
            "remux_only": job.remux_only,
            "output_names": outputs
                .iter()
                .map(|path| path.file_name().map(|name| name.to_string_lossy()))
                .collect::<Vec<_>>(),
            "dv": dv,
            "remaining": remaining,
        }),
    )
}

/// Read a JSON array of track indices. An absent key yields `current`; a
/// value that is not an array, or an unknown index, is an error.
fn valid_indices(
    body: &Value,
    key: &str,
    known: &[usize],
    current: &[usize],
) -> Result<Vec<usize>, String> {
    let Some(value) = body.get(key) else {
        return Ok(current.to_vec());
    };
    let Some(array) = value.as_array() else {
        return Err(format!("{key} must be an array"));
    };
    let mut out = Vec::with_capacity(array.len());
    for value in array {
        let Some(raw) = value.as_u64() else {
            return Err(format!("{key} must contain non-negative integers"));
        };
        let Ok(index) = usize::try_from(raw) else {
            return Err(format!("{key} contains an out-of-range index"));
        };
        if !known.contains(&index) {
            return Err(format!("{key} contains unknown track index {index}"));
        }
        out.push(index);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Read an optional JSON boolean. `None` when the key is absent; an error
/// when it holds anything but a boolean.
fn optional_bool(body: &Value, key: &str) -> Result<Option<bool>, String> {
    match body.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// Replace one job's track selection and per-job options.
#[allow(clippy::too_many_lines)]
pub fn job_tracks_set(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };

    let apply_remaining = match optional_bool(body, "apply_to_remaining") {
        Ok(value) => value.unwrap_or(false),
        Err(error) => return (400, json!({"error": error})),
    };

    let mut state = lock(shared);
    let encoder = state.config.encoder;
    let output_config = state.config.output.clone();

    let (selection, remux_only, dv_mode, audio_modes, subtitle_selected) = {
        let Some(job) = state.queue.job_by_id(id) else {
            return (404, json!({"error": "unknown job id"}));
        };
        if !tracks_editable(&state, id, job) {
            return (409, json!({"error": "job is encoding or already finished"}));
        }

        let audio_known: Vec<usize> = job.audio_tracks.iter().map(|t| t.index).collect();
        let subtitle_known: Vec<usize> = job.subtitle_tracks.iter().map(|t| t.index).collect();

        let current = &job.track_selection;
        let audio_indices =
            match valid_indices(body, "audio_indices", &audio_known, &current.audio_indices) {
                Ok(indices) => indices,
                Err(error) => return (400, json!({"error": error})),
            };
        // Kept a subset of the selection: the plan indexes per-stream codec
        // options by output position.
        let audio_to_opus =
            match valid_indices(body, "audio_to_opus", &audio_known, &current.audio_to_opus) {
                Ok(indices) => indices
                    .into_iter()
                    .filter(|i| audio_indices.contains(i))
                    .collect(),
                Err(error) => return (400, json!({"error": error})),
            };

        let subtitle_indices = match valid_indices(
            body,
            "subtitle_indices",
            &subtitle_known,
            &current.subtitle_indices,
        ) {
            Ok(indices) => indices,
            Err(error) => return (400, json!({"error": error})),
        };

        let selection = crate::tracks::TrackSelection {
            audio_indices,
            subtitle_indices,
            audio_to_opus,
        };

        // Both options are absent-means-unchanged, so a client that only knows
        // about tracks leaves them as they stand.
        let remux_only = match optional_bool(body, "remux_only") {
            Ok(value) => value.unwrap_or(job.remux_only),
            Err(error) => return (400, json!({"error": error})),
        };

        let dv_mode = match body.get("dv_mode") {
            None | Some(Value::Null) => job.dv_mode,
            Some(requested) => {
                let is_dv = job
                    .metadata
                    .as_ref()
                    .is_some_and(|meta| meta.hdr_type == HdrType::DolbyVision);
                if !is_dv {
                    return (400, json!({"error": "job is not a Dolby Vision source"}));
                }
                match requested.as_str() {
                    Some(DV_HDR10) => Some(DvMode::ToHdr10),
                    // Only SVT-AV1 can write the RPU.
                    Some(DV_KEEP) if encoder == Encoder::SvtAv1 => Some(DvMode::KeepDolbyVision),
                    // A stale keep under a hardware encoder cannot retain the
                    // RPU; rewrite to HDR10.
                    Some(DV_KEEP) if job.dv_mode == Some(DvMode::KeepDolbyVision) => {
                        Some(DvMode::ToHdr10)
                    }
                    Some(DV_KEEP) => {
                        return (
                            400,
                            json!({"error": format!(
                                "{} cannot write the Dolby Vision RPU",
                                encoder.display_name()
                            )}),
                        );
                    }
                    _ => return (400, json!({"error": "dv_mode must be 'keep' or 'hdr10'"})),
                }
            }
        };

        let audio_modes: Vec<Option<bool>> = job
            .audio_tracks
            .iter()
            .map(|track| {
                selection
                    .audio_indices
                    .contains(&track.index)
                    .then(|| selection.audio_to_opus.contains(&track.index))
            })
            .collect();
        let subtitle_selected: Vec<bool> = job
            .subtitle_tracks
            .iter()
            .map(|track| selection.subtitle_indices.contains(&track.index))
            .collect();

        (
            selection,
            remux_only,
            dv_mode,
            audio_modes,
            subtitle_selected,
        )
    };

    let job = state.queue.job_by_id_mut(id).expect("job checked above");
    let source_dv_profile = job.metadata.as_ref().and_then(|meta| meta.dv_profile);
    apply_track_config(job, selection, remux_only, dv_mode, &output_config, encoder);

    let mut applied = json!({
        "audio_indices": job.track_selection.audio_indices,
        "audio_to_opus": job.track_selection.audio_to_opus,
        "subtitle_indices": job.track_selection.subtitle_indices,
        "remux_only": job.remux_only,
        "dv_mode": job.dv_mode.map(dv_mode_name),
    });

    let mut applied_count = 1;
    if apply_remaining {
        applied_count += apply_to_remaining(
            &mut state.queue.state.jobs,
            &audio_modes,
            &subtitle_selected,
            dv_mode,
            source_dv_profile,
            &output_config,
            encoder,
        );
    }
    make_output_paths_unique(&mut state.queue.state.jobs);
    applied["applied"] = json!(applied_count);
    (200, applied)
}

/// Whether `path` sits inside `root`, comparing resolved paths so that `..`
/// segments and symlinks cannot step outside. An empty root allows everything.
pub fn within_root(path: &Path, root: &str) -> bool {
    confined_path(path, root).is_some()
}

/// The path to store: the spelling the user chose when unrestricted, and the
/// resolved path that was checked when under a browse root.
pub(crate) fn confined_path(path: &Path, root: &str) -> Option<PathBuf> {
    if root.is_empty() {
        return Some(path.to_path_buf());
    }
    let (Ok(path), Ok(root)) = (path.canonicalize(), PathBuf::from(root).canonicalize()) else {
        return None;
    };
    path.starts_with(root).then_some(path)
}

/// `path` resolved through its deepest existing ancestor, with the missing
/// components appended as written. `None` when a missing component is not a
/// plain name, or when no ancestor exists.
pub(crate) fn resolve_through_existing(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if let Ok(resolved) = ancestor.canonicalize() {
            let rest = path.strip_prefix(ancestor).ok()?;
            return rest
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
                .then(|| resolved.join(rest));
        }
    }
    None
}

/// Expand an add request into concrete video files and queue them.
#[allow(clippy::too_many_lines)]
pub fn queue_add(
    shared: &SharedState,
    probe_tx: &Sender<(u64, String)>,
    body: &Value,
    shutdown: &AtomicBool,
) -> (u16, Value) {
    if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
        return (503, json!({"error": "daemon is shutting down"}));
    }
    let Some(path) = body.get("path").and_then(Value::as_str) else {
        return (400, json!({"error": "missing 'path'"}));
    };
    let mode = body.get("mode").and_then(Value::as_str).unwrap_or("file");
    let _scan_guard = if mode == "folder_recursive" {
        match RecursiveScanGuard::acquire(shared) {
            Some(guard) => Some(guard),
            None => {
                return (
                    409,
                    json!({"error": "another recursive folder scan is already running"}),
                );
            }
        }
    } else {
        None
    };
    let path = PathBuf::from(path);
    // Confinement answers first; the existence check runs only on paths
    // already inside the browse root.
    let (browse_root, lang) = {
        let state = lock(shared);
        (
            state.config.daemon.browse_root.clone(),
            state.config.language,
        )
    };
    let Some(path) = confined_path(&path, &browse_root) else {
        return (
            403,
            json!({"error": "path is outside the configured browse root"}),
        );
    };
    if !path.exists() {
        return (400, json!({"error": "path does not exist"}));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    match mode {
        // A single file is queued whatever its name. The folder modes queue
        // every video file they find, including earlier outputs.
        "file" => {
            if !path.is_file() || !is_video_file(&path) {
                return (400, json!({"error": "not a video file"}));
            }
            files.push(path);
        }
        "folder" => {
            if !path.is_dir() {
                return (400, json!({"error": "not a directory"}));
            }
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    files.extend(
                        entries
                            .filter_map(Result::ok)
                            .map(|e| e.path())
                            .filter(|p| p.is_file() && is_video_file(p)),
                    );
                }
                Err(e) => {
                    return (
                        400,
                        json!({"error": format!("could not read directory: {e}")}),
                    );
                }
            }
        }
        "folder_recursive" => {
            if !path.is_dir() {
                return (400, json!({"error": "not a directory"}));
            }
            if browse_root.is_empty() {
                let _ = collect_video_files_cancellable_result(&path, &mut files, shutdown);
            } else {
                collect_video_files_within(
                    &path,
                    Path::new(&browse_root),
                    &mut files,
                    Some(shutdown),
                );
            }
            if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                return (503, json!({"error": "daemon is shutting down"}));
            }
        }
        other => return (400, json!({"error": format!("unknown mode '{other}'")})),
    }

    // Scanning follows symlinks, so a link can lead back out of the browse
    // root even from a folder inside it. Each result is re-checked.
    files = files
        .into_iter()
        .filter_map(|path| confined_path(&path, &browse_root))
        .collect();

    if files.is_empty() {
        return (
            400,
            json!({"error": crate::i18n::t(lang, Msg::NoVideoFiles)}),
        );
    }
    files.sort();

    let (added, already_queued, skipped) = super::add_paths(shared, probe_tx, files, &browse_root);
    (
        200,
        json!({
            "added": added,
            "already_queued": already_queued,
            "skipped": skipped,
        }),
    )
}

/// Keep at most one recursive scan in flight, leaving the other HTTP workers
/// free.
struct RecursiveScanGuard(SharedState);

impl RecursiveScanGuard {
    fn acquire(shared: &SharedState) -> Option<Self> {
        let mut state = lock(shared);
        if state.recursive_scan_active {
            return None;
        }
        state.recursive_scan_active = true;
        Some(Self(shared.clone()))
    }
}

impl Drop for RecursiveScanGuard {
    fn drop(&mut self) {
        lock(&self.0).recursive_scan_active = false;
    }
}

/// Remove a job unless its encode has already started.
pub fn queue_remove(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };
    let mut state = lock(shared);
    if state.queue.job_by_id(id).is_none() {
        return (404, json!({"error": "unknown job id"}));
    }
    if state.in_active_session(id) {
        return (409, json!({"error": "job is already encoding"}));
    }
    // A `Ripping` job is owned by the disc worker; removing it would leave
    // the extraction running with no job to land on.
    if state
        .queue
        .job_by_id(id)
        .is_some_and(|job| matches!(job.status, JobStatus::Ripping { .. }))
    {
        return (409, json!({"error": "job is being ripped from the disc"}));
    }
    let was_ready = state
        .queue
        .job_by_id(id)
        .is_some_and(|job| matches!(job.status, JobStatus::Ready));
    let staged = state
        .queue
        .job_by_id(id)
        .filter(|job| job.temporary)
        .map(|job| job.path.clone());
    state.queue.remove(id);
    if was_ready {
        let remaining_ready = state
            .queue
            .state
            .jobs
            .iter()
            .filter(|job| matches!(job.status, JobStatus::Ready))
            .count();
        state.queue.state.total_jobs_to_encode = state.queue.state.encoding_progress_done
            + usize::from(state.encoding_active)
            + remaining_ready;
    }
    let root = crate::disc::staging::staging_root(&state.config);
    drop(state);
    // Deletion runs with the lock released.
    if let Some(path) = staged {
        crate::disc::staging::discard_staged(&root, &path);
    }
    (200, json!({"ok": true}))
}

/// Raise a ready job by one queue position.
pub fn queue_move_up(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };
    let mut state = lock(shared);
    if state.queue.job_by_id(id).is_none() {
        return (404, json!({"error": "unknown job id"}));
    }
    if state.in_active_session(id) {
        return (409, json!({"error": "job is already encoding"}));
    }
    let moved = state.queue.move_ready_up(id);
    (200, json!({"moved": moved}))
}

/// Cancel the running encode session.
pub fn queue_cancel(shared: &SharedState) -> (u16, Value) {
    let mut state = lock(shared);
    if let Some(session) = state.session.as_ref().filter(|_| state.encoding_active) {
        session
            .cancel_flag
            .store(true, std::sync::atomic::Ordering::Release);
        let ready = state
            .queue
            .state
            .jobs
            .iter()
            .filter(|job| matches!(job.status, JobStatus::Ready))
            .count();
        state.queue.state.total_jobs_to_encode =
            state.queue.state.encoding_progress_done + 1 + ready;
        let mut skipped = 0;
        for job in &mut state.queue.state.jobs {
            if !matches!(job.status, JobStatus::Ready) {
                continue;
            }
            job.status = JobStatus::Skipped {
                reason: "Cancelled".to_string(),
            };
            skipped += 1;
        }
        state.queue.state.count_cancelled(skipped);
        state.queue.state.encoding_progress_done += skipped;
    }
    (200, json!({"ok": true}))
}

/// Stop in-flight probes. Jobs already configured or encoding are left alone.
pub fn queue_cancel_analysis(shared: &SharedState) -> (u16, Value) {
    let mut state = lock(shared);
    state
        .analysis_cancel
        .store(true, std::sync::atomic::Ordering::Release);
    let mut skipped = 0;
    for job in &mut state.queue.state.jobs {
        if matches!(job.status, JobStatus::Analyzing | JobStatus::Pending) {
            job.status = JobStatus::Skipped {
                reason: "Cancelled".to_string(),
            };
            skipped += 1;
        }
    }
    state.queue.state.count_cancelled(skipped);
    (200, json!({"ok": true, "skipped": skipped}))
}

/// Drop all jobs in terminal states. When one of them is a ripped title, its
/// rip is deleted too, and the request must carry `"confirm": true`; without
/// it the answer is 409 with `needs_confirm` and nothing is removed.
pub fn queue_clear_finished(shared: &SharedState, body: &Value) -> (u16, Value) {
    let mut state = lock(shared);
    let finished: Vec<(u64, Option<PathBuf>)> = state
        .queue
        .jobs_with_ids()
        .filter(|(_, job)| is_terminal(&job.status))
        .map(|(id, job)| (id, job.temporary.then(|| job.path.clone())))
        .collect();
    let confirmed = body.get("confirm").and_then(Value::as_bool) == Some(true);
    if !confirmed && finished.iter().any(|(_, path)| path.is_some()) {
        let lang = state.config.language;
        return (
            409,
            json!({
                "error": crate::i18n::t(lang, crate::i18n::Msg::WebClearFinishedRipPrompt),
                "needs_confirm": true,
            }),
        );
    }
    let removed = finished.len();
    let mut staged = Vec::new();
    for (id, path) in finished {
        state.queue.remove(id);
        staged.extend(path);
    }
    let root = crate::disc::staging::staging_root(&state.config);
    drop(state);
    // Deletion runs with the lock released.
    for path in staged {
        crate::disc::staging::discard_staged(&root, &path);
    }
    (200, json!({"removed": removed}))
}

/// Server-side file browser: list one directory level.
pub fn fs_browse(shared: &SharedState, path: &str, show_hidden: bool) -> (u16, Value) {
    let browse_root = lock(shared).config.daemon.browse_root.clone();
    let requested = if path.is_empty() {
        // With a root configured, that is where browsing starts.
        if browse_root.is_empty() {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map_or_else(|| PathBuf::from("/"), PathBuf::from)
        } else {
            PathBuf::from(&browse_root)
        }
    } else {
        PathBuf::from(path)
    };

    let Some(dir) = confined_path(&requested, &browse_root) else {
        return (
            403,
            json!({"error": "path is outside the configured browse root"}),
        );
    };

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (400, json!({"error": "cannot read directory"}));
    };

    let mut dirs: Vec<Value> = Vec::new();
    let mut files: Vec<Value> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let entry_path = entry.path();
        // is_dir/is_file follow symlinks, so linked media is listed like
        // anything else. One escaping the browse root is rejected on the way in.
        if entry_path.is_dir() {
            if within_root(&entry_path, &browse_root) {
                dirs.push(json!({"name": name, "symlink": entry_path.is_symlink()}));
            }
        } else if entry_path.is_file() && within_root(&entry_path, &browse_root) {
            let size = entry.metadata().ok().map(|m| m.len());
            files.push(json!({
                "name": name,
                "size": size,
                "is_video": is_video_file(&entry_path),
                "is_iso": crate::disc::is_iso(&entry_path),
            }));
        }
    }
    let by_name = |a: &Value, b: &Value| {
        let a = a["name"].as_str().unwrap_or("").to_lowercase();
        let b = b["name"].as_str().unwrap_or("").to_lowercase();
        a.cmp(&b)
    };
    dirs.sort_by(by_name);
    files.sort_by(by_name);

    (
        200,
        json!({
            "path": dir.to_string_lossy(),
            // No ".." out of the browse root.
            "parent": dir
                .parent()
                .filter(|parent| within_root(parent, &browse_root))
                .map(Path::to_string_lossy),
            "dirs": dirs,
            "files": files,
        }),
    )
}

/// Serialize the configuration with the auth token blanked out. The token
/// guards this endpoint and never travels back out of it.
fn redacted(config: &AppConfig) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or_else(|_| json!({}));
    if let Some(token) = value.pointer_mut("/daemon/auth_token") {
        *token = json!("");
    }
    value
}

/// The web UI's strings, resolved for the configured language: a flat
/// `key -> text` map of [`crate::i18n::WEB_KEYS`]. Not content-negotiated —
/// the language comes from the config, as it does for the TUI.
pub fn strings(shared: &SharedState) -> Value {
    let lang = lock(shared).config.language;
    let mut map: serde_json::Map<String, Value> = crate::i18n::WEB_KEYS
        .iter()
        .map(|(key, msg)| ((*key).to_string(), Value::from(crate::i18n::t(lang, *msg))))
        .collect();

    // The page sets `<html lang>` from this. Taken from the serde rename the
    // config file uses, so there is no second list of codes.
    let code = serde_json::to_value(lang)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "en".to_string());
    map.insert("html_lang".to_string(), Value::from(code));
    map.into()
}

// Disc ripping. The drive is attached to this machine; the browser only ever
// names ids this server handed out, never a path or a device.

/// Every drive `MakeMKV` reports, and the label of whatever is loaded. The ids
/// in the response are the only ones the other endpoints accept.
pub fn discs_list(shared: &SharedState) -> (u16, Value) {
    // The listing claims the drive the same way a scan or rip does: one
    // makemkvcon at a time.
    let config = {
        let mut state = lock(shared);
        if state
            .shutting_down
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return (503, json!({"error": "daemon is shutting down"}));
        }
        if state.disc.active {
            return (409, json!({"error": "a disc operation is already running"}));
        }
        state.disc.active = true;
        state.disc.listing = true;
        state.disc.error = None;
        state.disc.titles.clear();
        state.disc.disc_type = None;
        state.disc.scanned_source = None;
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        state.disc.cancel_flag = Some(cancel.clone());
        (state.config.clone(), cancel)
    };
    let (config, cancel) = config;
    let _claim = DiscClaim(shared.clone());

    let bin = match crate::disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => return disc_failure(&e, config.language),
    };
    // Listing takes a second or two and holds no lock.
    let drives = match crate::disc::list_drives(&bin, &cancel) {
        Ok(drives) => drives,
        Err(e) => return disc_failure(&e, config.language),
    };

    let payload: Vec<Value> = drives
        .iter()
        .map(|drive| {
            json!({
                "id": drive.id,
                "name": drive.name,
                "disc_label": drive.disc_label,
            })
        })
        .collect();
    lock(shared).disc.drives = drives;
    (200, json!({ "drives": payload }))
}

/// Holds `disc.active` for the duration of a drive listing.
struct DiscClaim(SharedState);

impl Drop for DiscClaim {
    fn drop(&mut self) {
        let mut state = lock(&self.0);
        state.disc.active = false;
        state.disc.listing = false;
        state.disc.cancel_flag = None;
    }
}

/// Start scanning a drive (`{"drive": N}`) or a disc folder on disk
/// (`{"folder": "<path>"}`). The titles arrive in `/api/status`.
pub fn discs_scan(shared: &SharedState, disc_tx: &Sender<DiscEvent>, body: &Value) -> (u16, Value) {
    // The folder and binary checks touch the filesystem and run with no lock
    // held, against a snapshot.
    let (config, drives) = {
        let state = lock(shared);
        if state.disc.active {
            return (409, json!({"error": "a disc operation is already running"}));
        }
        (state.config.clone(), state.disc.drives.clone())
    };

    let source = if let Some(folder) = body.get("folder").and_then(Value::as_str) {
        // The same boundary the file browser enforces: a client cannot read a
        // disc from outside the browse root.
        let Some(path) = confined_path(Path::new(folder), &config.daemon.browse_root) else {
            return (
                403,
                json!({"error": "path is outside the configured browse root"}),
            );
        };
        match crate::disc::DiscSource::folder(path) {
            Ok(source) => source,
            Err(e) => return disc_failure(&e, config.language),
        }
    } else {
        let Some(drive) = body.get("drive").and_then(Value::as_u64).and_then(as_id) else {
            return (400, json!({"error": "missing or invalid 'drive'"}));
        };
        let Some(disc_drive) = drives.into_iter().find(|known| known.id == drive) else {
            return (400, json!({"error": "unknown drive id"}));
        };
        crate::disc::DiscSource::Drive(disc_drive)
    };

    let bin = match crate::disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => return disc_failure(&e, config.language),
    };

    let mut state = lock(shared);
    if state
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return (503, json!({"error": "daemon is shutting down"}));
    }
    if state.disc.active {
        return (409, json!({"error": "a disc operation is already running"}));
    }
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(previous) = state.disc_worker.take() {
        // `active` was false, so the previous run has settled; join before
        // arming the next one so shutdown always sees a live handle.
        let _ = previous.join();
    }
    state.disc.cancel_flag = Some(cancel.clone());
    state.disc.active = true;
    state.disc.scanning = true;
    state.disc.error = None;
    state.disc.titles.clear();
    state.disc.disc_type = None;
    state.disc.scanned_source = Some(source.clone());
    state.disc_worker = Some(crate::disc::worker::spawn_scan(
        bin,
        source,
        &cancel,
        disc_tx.clone(),
    ));
    (200, json!({"ok": true}))
}

/// Extract the named titles, one after another, into the staging directory.
#[allow(clippy::too_many_lines)]
pub fn discs_rip(shared: &SharedState, disc_tx: &Sender<DiscEvent>, body: &Value) -> (u16, Value) {
    let Some(requested) = body.get("titles").and_then(Value::as_array) else {
        return (400, json!({"error": "missing 'titles'"}));
    };
    let ids: Option<Vec<u32>> = requested
        .iter()
        .map(|value| value.as_u64().and_then(as_id))
        .collect();
    let Some(ids) = ids.filter(|ids| !ids.is_empty()) else {
        return (
            400,
            json!({"error": "'titles' must be a non-empty list of ids"}),
        );
    };

    // The folder, destination and binary checks touch the filesystem and run
    // with no lock held, against a snapshot.
    let (scanned_source, scanned_titles, config) = {
        let state = lock(shared);
        if state.disc.active {
            return (409, json!({"error": "a disc operation is already running"}));
        }
        (
            state.disc.scanned_source.clone(),
            state.disc.titles.clone(),
            state.config.clone(),
        )
    };
    let Some(source) = scanned_source.filter(|scanned| match scanned {
        crate::disc::DiscSource::Drive(drive) => {
            body.get("drive").and_then(Value::as_u64).and_then(as_id) == Some(drive.id)
        }
        crate::disc::DiscSource::Folder(path) => body
            .get("folder")
            .and_then(Value::as_str)
            .and_then(|folder| confined_path(Path::new(folder), &config.daemon.browse_root))
            .is_some_and(|folder| folder == *path),
    }) else {
        return (
            400,
            json!({"error": "scan the disc before ripping from it"}),
        );
    };
    // Only titles this server reported, and each of them once.
    let mut titles = Vec::new();
    for id in &ids {
        let Some(title) = scanned_titles.iter().find(|title| title.id == *id) else {
            return (400, json!({"error": format!("unknown title id {id}")}));
        };
        if titles
            .iter()
            .any(|kept: &crate::disc::DiscTitle| kept.id == *id)
        {
            return (
                400,
                json!({"error": format!("title id {id} is listed twice")}),
            );
        }
        titles.push(title.clone());
    }
    if let Err(e) = crate::disc::staging::require_destination(&config) {
        return disc_failure(&e, config.language);
    }
    let bin = match crate::disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => return disc_failure(&e, config.language),
    };

    let mut state = lock(shared);
    if state
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return (503, json!({"error": "daemon is shutting down"}));
    }
    if state.disc.active {
        return (409, json!({"error": "a disc operation is already running"}));
    }
    if state.disc.scanned_source.as_ref() != Some(&source) {
        return (
            400,
            json!({"error": "scan the disc before ripping from it"}),
        );
    }
    let config = state.config.clone();
    if config
        .output
        .output_directory
        .as_deref()
        .is_none_or(|dir| dir.trim().is_empty())
    {
        return disc_failure(&crate::disc::DiscError::NoDestination, config.language);
    }

    state.queue.state.reset_session_if_finished();

    // Each title is a queue job from the start, so a rip renders in the queue
    // table like everything else. Its path is the title's name until the file
    // it extracts to is known.
    let job_ids: Vec<u64> = titles
        .iter()
        .map(|title| {
            let mut job = EncodingJob::new(PathBuf::from(title.name.clone()));
            job.status = JobStatus::Ripping { progress: 0.0 };
            job.temporary = true;
            state.queue.push(job)
        })
        .collect();

    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(previous) = state.disc_worker.take() {
        let _ = previous.join();
    }
    state.disc.cancel_flag = Some(cancel.clone());
    state.disc.active = true;
    state.disc.scanning = false;
    state.disc.error = None;
    state.disc.job_ids.clone_from(&job_ids);
    state.disc_worker = Some(crate::disc::worker::spawn_rips(
        bin,
        config,
        source,
        titles,
        &cancel,
        disc_tx.clone(),
    ));
    (200, json!({"ok": true, "jobs": job_ids}))
}

/// Stop the running scan or rip. The run reports its own cancellation.
pub fn discs_cancel(shared: &SharedState) -> (u16, Value) {
    lock(shared).disc.cancel();
    (200, json!({"ok": true}))
}

/// Ids are `u32` on the wire; anything wider was never handed out.
fn as_id(value: u64) -> Option<u32> {
    u32::try_from(value).ok()
}

/// A disc failure in the user's language, with the status that fits it.
fn disc_failure(error: &crate::disc::DiscError, lang: crate::i18n::Language) -> (u16, Value) {
    let status = match error {
        crate::disc::DiscError::NotInstalled
        | crate::disc::DiscError::NoDrive
        | crate::disc::DiscError::DriveEmpty
        | crate::disc::DiscError::NoDestination
        | crate::disc::DiscError::NotADiscFolder => 400,
        crate::disc::DiscError::PermissionDenied => 403,
        crate::disc::DiscError::Cancelled => 409,
        _ => 500,
    };
    (status, json!({"error": error.message(lang)}))
}

/// Read the full configuration, minus the auth token.
pub fn settings_get(shared: &SharedState) -> Value {
    redacted(&lock(shared).config)
}

/// Report host-level settings capabilities for the current connection.
pub fn settings_access(shared: &SharedState, local_request: bool) -> Value {
    json!({
        "local": local_request,
        "autostart_supported": crate::daemon::service::supported(),
        "autostart": crate::daemon::service::installed(),
        "auth_token_set": !lock(shared).config.daemon.auth_token.is_empty(),
        "setting_paths": crate::config::settings::SERIALIZED_SETTING_PATHS,
        "preset_tables": preset_tables(),
        "local_only_paths": crate::config::settings::LOCAL_ONLY_SETTING_PATHS,
    })
}

/// The per-tier values each named quality preset writes into the config.
fn preset_tables() -> Value {
    let mut tables = serde_json::Map::new();
    for preset in crate::config::QualityPreset::ALL {
        if let Some(tables_for) = preset.presets() {
            let name = serde_json::to_value(preset).unwrap_or(Value::Null);
            if let Some(name) = name.as_str() {
                tables.insert(
                    name.to_string(),
                    serde_json::to_value(tables_for).unwrap_or(Value::Null),
                );
            }
        }
    }
    Value::Object(tables)
}

/// Overlay `patch` onto `base`: objects merge key by key, and any other value
/// replaces what `base` holds.
fn overlay_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                overlay_json(base.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

/// Merge client settings onto the live config, keeping live values for keys
/// the client leaves out and host-sensitive fields for remote requests.
fn merged_settings(
    body: &Value,
    live: &AppConfig,
    local_request: bool,
) -> Result<AppConfig, String> {
    let mut merged = serde_json::to_value(live).map_err(|e| format!("invalid settings: {e}"))?;
    overlay_json(&mut merged, body);
    let mut config: AppConfig =
        serde_json::from_value(merged).map_err(|e| format!("invalid settings: {e}"))?;
    if local_request {
        config.daemon.browse_root = config.daemon.browse_root.trim().to_string();
        config.daemon.auth_token = config.daemon.auth_token.trim().to_string();
        if config.daemon.auth_token.is_empty() {
            config.daemon.auth_token.clone_from(&live.daemon.auth_token);
        }
        config.disc.makemkvcon_path = config
            .disc
            .makemkvcon_path
            .take()
            .and_then(|path| (!path.trim().is_empty()).then(|| path.trim().to_string()));
        config.disc.staging_directory = config
            .disc
            .staging_directory
            .take()
            .and_then(|path| (!path.trim().is_empty()).then(|| path.trim().to_string()));
        config.normalize_changed_host_paths(live)?;
    } else {
        config.daemon = live.daemon.clone();
        config.disc = live.disc.clone();
    }
    config.validate_settings()?;
    config.sanitize();
    let directory = config
        .output
        .output_directory
        .as_deref()
        .filter(|dir| !dir.is_empty());
    if directory.is_none() && !config.output.same_directory {
        return Err(crate::i18n::t(config.language, Msg::OutputDirectoryInvalid).to_string());
    }
    // Ripped files encode into the output directory whatever `same_directory` says.
    // A directory unchanged from the live config, under the same browse root, is
    // kept as is; `settings_post` warns when it no longer exists.
    if let Some(directory) = directory
        && (live.output.output_directory.as_deref() != Some(directory)
            || live.daemon.browse_root != config.daemon.browse_root)
    {
        let root = &config.daemon.browse_root;
        let Some(path) = confined_path(Path::new(directory), root).filter(|path| path.is_dir())
        else {
            let msg = if root.is_empty() {
                Msg::OutputDirectoryInvalid
            } else {
                Msg::OutputDirectoryOutsideBrowseRoot
            };
            return Err(crate::i18n::t(config.language, msg).to_string());
        };
        config.output.output_directory = Some(path.to_string_lossy().into_owned());
    }
    Ok(config)
}

/// Write `live` back to `config.toml` after a refused save: 409 with `error`,
/// or 500 when `live` cannot be written.
fn restore_live_settings(live: &AppConfig, error: &str) -> (u16, Value) {
    match live.save() {
        Ok(()) => (409, json!({"error": error})),
        Err(e) => {
            tracing::error!("Refused settings remain in config.toml; restoring failed: {e}");
            (
                500,
                json!({"error": format!(
                    "{}: {e}",
                    crate::i18n::t(live.language, Msg::SettingsRestoreFailed)
                )}),
            )
        }
    }
}

/// Held by settings writes from the snapshot of the live config through its
/// save and commit.
static SETTINGS_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Why `config` cannot replace `live` while `jobs` — unfinished jobs as
/// `(source, output, temporary)` — are queued. `inside` tells whether a path
/// lies within a browse root.
fn queue_conflict(
    live: &AppConfig,
    config: &AppConfig,
    jobs: &[(PathBuf, Option<PathBuf>, bool)],
    inside: impl Fn(&Path, &str) -> bool,
) -> Option<&'static str> {
    let root = &config.daemon.browse_root;
    if live.daemon.browse_root != *root
        && jobs.iter().any(|(path, output, temporary)| {
            (!temporary && !inside(path, root))
                || output
                    .as_deref()
                    .and_then(Path::parent)
                    .is_some_and(|parent| !inside(parent, root))
        })
    {
        return Some(crate::i18n::t(config.language, Msg::BrowseRootExcludesJobs));
    }
    let has_directory = |config: &AppConfig| {
        config
            .output
            .output_directory
            .as_deref()
            .is_some_and(|dir| !dir.trim().is_empty())
    };
    if live.disc.staging_directory != config.disc.staging_directory
        && jobs.iter().any(|(_, _, temporary)| *temporary)
    {
        return Some(crate::i18n::t(
            config.language,
            Msg::QueuedRipsPinStagingDirectory,
        ));
    }
    (has_directory(live)
        && !has_directory(config)
        && jobs.iter().any(|(_, _, temporary)| *temporary))
    .then(|| crate::i18n::t(config.language, Msg::QueuedRipsNeedOutputDirectory))
}

/// Replace the configuration: sanitize, persist to config.toml, and swap the
/// live copy. Changes apply from the next analysis/encode.
pub fn settings_post(shared: &SharedState, body: &Value, local_request: bool) -> (u16, Value) {
    let _writing = SETTINGS_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Merging, the browse-root check and the config write all touch the
    // filesystem and run with no lock held, against a snapshot of the live
    // config and the queued paths.
    let (live, job_paths, bound_publicly) = {
        let state = lock(shared);
        let paths: Vec<(PathBuf, Option<PathBuf>, bool)> = state
            .queue
            .state
            .jobs
            .iter()
            // Finished jobs may have had their source deleted.
            .filter(|job| !is_terminal(&job.status))
            .map(|job| (job.path.clone(), job.output_path.clone(), job.temporary))
            .collect();
        (state.config.clone(), paths, state.bound_publicly)
    };
    let config = match merged_settings(body, &live, local_request) {
        Ok(config) => config,
        Err(e) => return (400, json!({"error": e})),
    };
    if bound_publicly && config.daemon.browse_root.is_empty() {
        return (
            400,
            json!({"error": crate::i18n::t(config.language, Msg::BrowseRootRequired)}),
        );
    }
    if let Some(error) = queue_conflict(&live, &config, &job_paths, within_root) {
        return (409, json!({"error": error}));
    }
    let encoder_available = (config.encoder != live.encoder)
        .then(|| crate::utils::DependencyStatus::encoder_available(config.encoder.ffmpeg_name()));
    if let Err(e) = config.save() {
        return (500, json!({"error": format!("failed to save: {e}")}));
    }
    let output_directory_missing = config
        .output
        .output_directory
        .as_deref()
        .is_some_and(|dir| !dir.is_empty() && !Path::new(dir).is_dir());
    let mut state = lock(shared);
    // Jobs queued since the snapshot are compared by path prefix, without
    // touching the filesystem.
    let queued_since: Vec<(PathBuf, Option<PathBuf>, bool)> = state
        .queue
        .state
        .jobs
        .iter()
        .filter(|job| {
            !is_terminal(&job.status) && !job_paths.iter().any(|(path, ..)| *path == job.path)
        })
        .map(|job| (job.path.clone(), job.output_path.clone(), job.temporary))
        .collect();
    if let Some(error) = queue_conflict(&live, &config, &queued_since, |path, root| {
        root.is_empty() || path.starts_with(root)
    }) {
        drop(state);
        return restore_live_settings(&live, error);
    }
    let output_changed = state.config.output != config.output;
    let mut saved = redacted(&config);
    if output_directory_missing {
        saved["_warning"] = json!(crate::i18n::t(
            config.language,
            crate::i18n::Msg::OutputDirectoryMissing
        ));
    }
    state.config = config;
    if let Some(available) = encoder_available {
        state.deps.encoder = available;
    }
    if output_changed {
        let output = state.config.output.clone();
        for job in &mut state.queue.state.jobs {
            if matches!(job.status, JobStatus::AwaitingConfig | JobStatus::Ready) {
                job.generate_output_path(&output);
            }
        }
        make_output_paths_unique(&mut state.queue.state.jobs);
    }
    (200, saved)
}

/// Install or remove the user-level daemon autostart service.
pub fn settings_service_post(
    shared: &SharedState,
    body: &Value,
    local_request: bool,
) -> (u16, Value) {
    if !local_request {
        return (
            403,
            json!({"error": "autostart can only be changed locally"}),
        );
    }
    if !crate::daemon::service::supported() {
        return (
            400,
            json!({"error": "autostart is not supported on this platform"}),
        );
    }
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return (400, json!({"error": "enabled must be a boolean"}));
    };
    let _writing = SETTINGS_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The config write runs with no lock held.
    let enabled_config = if enabled {
        let config = {
            let state = lock(shared);
            if state.config.daemon.enabled {
                None
            } else {
                let mut config = state.config.clone();
                config.daemon.enabled = true;
                Some(config)
            }
        };
        match config {
            None => false,
            Some(config) => {
                if let Err(error) = config.save() {
                    return (
                        500,
                        json!({"error": format!("failed to enable daemon: {error}")}),
                    );
                }
                lock(shared).config.daemon.enabled = true;
                true
            }
        }
    } else {
        false
    };
    let result = if enabled {
        crate::daemon::service::install().map(|_| ())
    } else {
        crate::daemon::service::uninstall_keep_running()
    };
    match result {
        Ok(()) => (
            200,
            json!({
                "enabled": crate::daemon::service::installed(),
                "daemon_enabled": lock(shared).config.daemon.enabled,
            }),
        ),
        Err(error) => {
            if enabled_config {
                let mut config = lock(shared).config.clone();
                config.daemon.enabled = false;
                if config.save().is_ok() {
                    lock(shared).config.daemon.enabled = false;
                }
            }
            (500, json!({"error": error.to_string()}))
        }
    }
}

/// Serialize a job status as a tagged JSON object.
fn status_json(status: &JobStatus) -> Value {
    match status {
        JobStatus::Pending => json!({"kind": "pending"}),
        JobStatus::Analyzing => json!({"kind": "analyzing"}),
        JobStatus::AwaitingConfig => json!({"kind": "awaiting_config"}),
        JobStatus::Ready => json!({"kind": "ready"}),
        JobStatus::Encoding { progress } => json!({"kind": "encoding", "progress": progress}),
        JobStatus::Ripping { progress } => json!({"kind": "ripping", "progress": progress}),
        JobStatus::Verifying => json!({"kind": "verifying"}),
        JobStatus::Done => json!({"kind": "done"}),
        JobStatus::DoneWithVmaf { score } => json!({"kind": "done_vmaf", "vmaf": score}),
        JobStatus::DoneVmafFailed { reason } => {
            json!({"kind": "done_vmaf_failed", "reason": reason})
        }
        JobStatus::Skipped { reason } => json!({"kind": "skipped", "reason": reason}),
        JobStatus::Error { message } => json!({"kind": "error", "message": message}),
        JobStatus::QualityWarning {
            vmaf,
            min_score,
            threshold,
        } => {
            json!({
                "kind": "quality_warning",
                "vmaf": vmaf,
                "min_score": min_score,
                "threshold": threshold,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::queue_add;
    use super::{
        RecursiveScanGuard, queue, queue_cancel, queue_clear_finished, queue_move_up, queue_remove,
        settings_access, status, within_root,
    };
    use crate::config::AppConfig;
    #[cfg(unix)]
    use crate::config::DaemonConfig;
    use crate::daemon::state::{DaemonState, EncodeSession, lock};
    use crate::queue::{EncodingJob, JobStatus};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    #[test]
    fn settings_access_carries_the_named_preset_tables() {
        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));

        let body = settings_access(&shared, true);

        let expected = serde_json::to_value(
            crate::config::QualityPreset::High
                .presets()
                .expect("high is a named preset"),
        )
        .unwrap();
        assert_eq!(body["preset_tables"]["high"], expected);
        assert_eq!(body["preset_tables"]["custom"], serde_json::Value::Null);
    }

    #[test]
    fn clearing_a_finished_rip_needs_confirmation() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut job = EncodingJob::new(PathBuf::from("/nonexistent/rip-1-ab/title_t00.mkv"));
        job.status = JobStatus::DoneWithVmaf { score: 96.0 };
        job.temporary = true;
        state.queue.push(job);
        let shared = Arc::new(Mutex::new(state));

        let (code, body) = queue_clear_finished(&shared, &serde_json::json!({}));
        assert_eq!(code, 409);
        assert_eq!(body["needs_confirm"], true);
        assert_eq!(lock(&shared).queue.state.jobs.len(), 1);

        let (code, body) = queue_clear_finished(&shared, &serde_json::json!({"confirm": true}));
        assert_eq!(code, 200);
        assert_eq!(body["removed"], 1);
        assert!(lock(&shared).queue.state.jobs.is_empty());
    }

    #[test]
    fn clearing_finished_files_needs_no_confirmation() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut job = EncodingJob::new(PathBuf::from("/x/in.mkv"));
        job.status = JobStatus::DoneWithVmaf { score: 96.0 };
        state.queue.push(job);
        let shared = Arc::new(Mutex::new(state));

        let (code, body) = queue_clear_finished(&shared, &serde_json::json!({}));
        assert_eq!(code, 200);
        assert_eq!(body["removed"], 1);
    }

    #[test]
    fn queue_rows_carry_crf_output_name_and_source_kept() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut job = EncodingJob::new(PathBuf::from("/x/in.mkv"));
        job.status = JobStatus::DoneWithVmaf { score: 96.0 };
        job.crf = Some(30);
        job.source_kept_vmaf = Some(91.0);
        job.output_path = Some(PathBuf::from("/x/out.mkv"));
        state.queue.push(job);
        let shared = Arc::new(Mutex::new(state));
        let row = &queue(&shared)["jobs"][0];
        assert_eq!(row["crf"], 30);
        assert_eq!(row["source_kept_vmaf"], 91.0);
        assert_eq!(row["output_name"], "out.mkv");
        assert!(row["quality"].is_string());
    }

    #[test]
    fn status_reports_dependencies_and_vmaf() {
        let mut state = DaemonState::new(AppConfig::default());
        state.deps.vmaf = false;
        state.deps.encoder = false;
        let threshold = state.config.quality.vmaf_threshold;
        let shared = Arc::new(Mutex::new(state));
        let value = status(&shared);
        assert_eq!(value["deps"]["vmaf"], false);
        assert_eq!(value["deps"]["encoder"], false);
        assert_eq!(value["deps"]["opus"], true);
        assert_eq!(value["vmaf_threshold"], threshold);
        // The web UI reads a drop in this value as a restart.
        assert!(value["uptime_secs"].is_u64());
        // The web UI reloads its strings when this stops matching its own.
        assert_eq!(value["language"], "en");
    }

    /// Ripping, analyzing, verifying and encoding each report as the current job.
    #[test]
    fn every_working_phase_is_reported_as_current() {
        for (phase, kind) in [
            (JobStatus::Ripping { progress: 12.5 }, "ripping"),
            (JobStatus::Analyzing, "analyzing"),
            (JobStatus::Verifying, "verifying"),
            (JobStatus::Encoding { progress: 40.0 }, "encoding"),
        ] {
            let mut state = DaemonState::new(AppConfig::default());
            let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
            job.status = phase;
            state.queue.push(job);
            let shared = Arc::new(Mutex::new(state));

            let current = &status(&shared)["current"];
            assert_eq!(current["filename"], "movie.mkv", "for {kind}");
            assert_eq!(current["status"]["kind"], kind);
        }
    }

    /// A queue holding only non-working states reports no current job.
    #[test]
    fn a_waiting_queue_has_no_current_job() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
        job.status = JobStatus::AwaitingConfig;
        state.queue.push(job);
        let shared = Arc::new(Mutex::new(state));

        assert!(status(&shared)["current"].is_null());
    }

    /// Both counts are session counters: they hold after the rows are cleared.
    #[test]
    fn cancelled_jobs_are_not_counted_again_as_skipped() {
        let mut state = DaemonState::new(AppConfig::default());
        state.queue.state.skipped_count = 1;
        state.queue.state.count_cancelled(1);
        let shared = Arc::new(Mutex::new(state));

        let counts = &status(&shared)["counts"];
        assert_eq!(counts["cancelled"], 1);
        assert_eq!(counts["skipped"], 1);
    }

    #[test]
    fn queue_rows_close_tracks_of_a_job_in_the_running_session() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut started = EncodingJob::new(PathBuf::from("/tmp/started.mkv"));
        started.status = JobStatus::Ready;
        let started_id = state.queue.push(started);
        let mut waiting = EncodingJob::new(PathBuf::from("/tmp/waiting.mkv"));
        waiting.status = JobStatus::Ready;
        let waiting_id = state.queue.push(waiting);
        state.session = Some(EncodeSession {
            job_ids: vec![started_id],
            cancel_flag: Arc::new(AtomicBool::new(false)),
        });
        state.encoding_active = true;
        let shared = Arc::new(Mutex::new(state));

        let body = queue(&shared);
        let editable = |id: u64| {
            body["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap()["tracks_editable"]
                .clone()
        };

        assert_eq!(editable(started_id), serde_json::json!(false));
        assert_eq!(editable(waiting_id), serde_json::json!(true));
    }

    #[test]
    fn a_waiting_session_job_can_be_removed() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut active = EncodingJob::new(PathBuf::from("/tmp/active.mkv"));
        active.status = JobStatus::Encoding { progress: 10.0 };
        let active_id = state.queue.push(active);
        let mut waiting = EncodingJob::new(PathBuf::from("/tmp/waiting.mkv"));
        waiting.status = JobStatus::Ready;
        let waiting_id = state.queue.push(waiting);
        state.session = Some(EncodeSession {
            job_ids: vec![active_id],
            cancel_flag: Arc::new(AtomicBool::new(false)),
        });
        state.encoding_active = true;
        state.queue.state.total_jobs_to_encode = 2;
        let shared = Arc::new(Mutex::new(state));

        let (code, body) = queue_remove(&shared, &serde_json::json!({"id": waiting_id}));

        assert_eq!((code, body), (200, serde_json::json!({"ok": true})));
        let state = lock(&shared);
        assert!(state.queue.job_by_id(waiting_id).is_none());
        assert_eq!(state.queue.state.total_jobs_to_encode, 1);
        drop(state);

        let (code, body) = queue_remove(&shared, &serde_json::json!({"id": active_id}));
        assert_eq!(code, 409);
        assert_eq!(body["error"], "job is already encoding");
    }

    #[test]
    fn cancelling_an_encode_also_settles_ready_jobs() {
        let mut state = DaemonState::new(AppConfig::default());
        let mut active = EncodingJob::new(PathBuf::from("active.mkv"));
        active.status = JobStatus::Encoding { progress: 10.0 };
        let active_id = state.queue.push(active);
        let mut waiting = EncodingJob::new(PathBuf::from("waiting.mkv"));
        waiting.status = JobStatus::Ready;
        let waiting_id = state.queue.push(waiting);
        let cancelled = Arc::new(AtomicBool::new(false));
        state.session = Some(EncodeSession {
            job_ids: vec![active_id],
            cancel_flag: cancelled.clone(),
        });
        state.encoding_active = true;
        let shared = Arc::new(Mutex::new(state));

        queue_cancel(&shared);

        let state = lock(&shared);
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
        assert!(matches!(
            state.queue.job_by_id(waiting_id).unwrap().status,
            JobStatus::Skipped { .. }
        ));
        assert_eq!(state.queue.state.skipped_count, 1);
        assert_eq!(state.queue.state.cancelled_count, 1);
    }

    #[test]
    fn a_ready_job_moves_up_with_its_stable_id() {
        let mut state = DaemonState::new(AppConfig::default());
        for name in ["first.mkv", "second.mkv"] {
            let mut job = EncodingJob::new(PathBuf::from(name));
            job.status = JobStatus::Ready;
            state.queue.push(job);
        }
        let shared = Arc::new(Mutex::new(state));

        let before = queue(&shared);
        let second_id = before["jobs"][1]["id"].as_u64().unwrap();
        assert_eq!(before["jobs"][1]["can_move_up"], true);
        assert_eq!(
            queue_move_up(&shared, &serde_json::json!({"id": second_id})),
            (200, serde_json::json!({"moved": true}))
        );

        let after = queue(&shared);
        assert_eq!(after["jobs"][0]["id"], second_id);
        assert_eq!(after["jobs"][0]["filename"], "second.mkv");
        assert_eq!(after["jobs"][0]["can_move_up"], false);
    }

    /// An empty root allows everything; a configured one confines to itself.
    #[test]
    fn browse_root_confines_paths() {
        let dir = std::env::temp_dir();
        let root = dir.join("av1c_root_test");
        let inside = root.join("inside");
        std::fs::create_dir_all(&inside).unwrap();

        let root_str = root.to_string_lossy().into_owned();
        assert!(within_root(&inside, &root_str));
        assert!(within_root(&root, &root_str));
        assert!(within_root(&inside, ""));

        // Traversal is resolved before the comparison, so it cannot escape.
        assert!(!within_root(&dir, &root_str));
        assert!(!within_root(&root.join("..").join(".."), &root_str));

        #[cfg(unix)]
        {
            let outside = dir.join(format!("av1c_outside_{}", std::process::id()));
            std::fs::write(&outside, b"x").unwrap();
            let link = root.join("outside.mkv");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(!within_root(&link, &root_str));
            let _ = std::fs::remove_file(outside);
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A path that cannot be resolved is refused rather than assumed safe.
    #[test]
    fn unresolvable_paths_are_refused_under_a_root() {
        assert!(!within_root(Path::new("/nonexistent/x.mkv"), "/tmp"));
    }

    #[cfg(unix)]
    #[test]
    fn queued_paths_keep_the_resolved_target_that_passed_confinement() {
        let base = std::env::temp_dir().join(format!("av1c_queue_link_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let source = base.join("movie.mkv");
        let link = base.join("alias.mkv");
        std::fs::write(&source, b"video").unwrap();
        std::os::unix::fs::symlink(&source, &link).unwrap();

        let config = AppConfig {
            daemon: DaemonConfig {
                browse_root: base.to_string_lossy().into_owned(),
                ..DaemonConfig::default()
            },
            ..AppConfig::default()
        };
        let shared = Arc::new(Mutex::new(DaemonState::new(config)));
        let (tx, rx) = std::sync::mpsc::channel();
        let (status, _) = queue_add(
            &shared,
            &tx,
            &serde_json::json!({"path": link, "mode": "file"}),
            &std::sync::atomic::AtomicBool::new(false),
        );

        assert_eq!(status, 200);
        assert_eq!(
            lock(&shared).queue.state.jobs[0].path,
            source.canonicalize().unwrap()
        );
        assert_eq!(
            std::path::PathBuf::from(rx.recv().unwrap().1),
            source.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(base);
    }

    mod discs {
        use super::super::*;
        use crate::daemon::state::DaemonState;
        use crate::disc::{DiscDrive, DiscTitle};
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::Duration;

        /// A daemon that has listed one drive and scanned it, as a client must
        /// have done before it can ask for anything else.
        fn scanned(output_directory: Option<String>) -> SharedState {
            let config = AppConfig {
                output: crate::config::OutputConfig {
                    output_directory,
                    ..crate::config::OutputConfig::default()
                },
                ..AppConfig::default()
            };
            let shared: SharedState = Arc::new(Mutex::new(DaemonState::new(config)));
            {
                let mut state = lock(&shared);
                state.disc.drives = vec![DiscDrive {
                    id: 0,
                    name: "BD-RE".to_string(),
                    disc_label: Some("DISC".to_string()),
                }];
                state.disc.scanned_source =
                    Some(crate::disc::DiscSource::Drive(state.disc.drives[0].clone()));
                state.disc.titles = vec![DiscTitle {
                    id: 3,
                    name: "Feature".to_string(),
                    duration: Duration::from_mins(90),
                    size_bytes: 1024,
                    chapters: 12,
                    tracks: vec!["Video AVC 1920x1080".to_string()],
                }];
            }
            shared
        }

        fn rip(shared: &SharedState, body: &Value) -> (u16, Value) {
            let (tx, _rx) = mpsc::channel();
            discs_rip(shared, &tx, body)
        }

        /// Ids the server never handed out are refused before `MakeMKV` is
        /// ever consulted.
        #[test]
        fn only_ids_the_server_issued_are_accepted() {
            let shared = scanned(Some("/tmp".to_string()));
            let (tx, _rx) = mpsc::channel();

            assert_eq!(discs_scan(&shared, &tx, &json!({"drive": 7})).0, 400);
            assert_eq!(discs_scan(&shared, &tx, &json!({})).0, 400);
            assert_eq!(discs_scan(&shared, &tx, &json!({"drive": -1})).0, 400);

            assert_eq!(rip(&shared, &json!({"drive": 7, "titles": [3]})).0, 400);
            assert_eq!(rip(&shared, &json!({"drive": 0, "titles": [9]})).0, 400);
            assert_eq!(rip(&shared, &json!({"drive": 0, "titles": []})).0, 400);
            assert_eq!(rip(&shared, &json!({"drive": 0, "titles": [3, 3]})).0, 400);
            assert_eq!(
                rip(&shared, &json!({"drive": 0, "titles": ["../../etc"]})).0,
                400
            );
            // Nothing was started, and no job was queued on the way out.
            assert!(!lock(&shared).disc.active);
            assert!(lock(&shared).queue.state.jobs.is_empty());
        }

        /// One drive, one run.
        #[test]
        fn a_second_run_is_refused_while_one_is_active() {
            let shared = scanned(Some("/tmp".to_string()));
            lock(&shared).disc.active = true;
            let (tx, _rx) = mpsc::channel();

            assert_eq!(discs_list(&shared).0, 409);
            assert_eq!(discs_scan(&shared, &tx, &json!({"drive": 0})).0, 409);
            assert_eq!(rip(&shared, &json!({"drive": 0, "titles": [3]})).0, 409);
        }

        /// Without a destination the encode would be written into the staging
        /// directory that is deleted afterwards, so the rip never starts.
        #[test]
        fn a_rip_without_a_destination_is_refused_with_the_reason() {
            let shared = scanned(None);
            let (status, body) = rip(&shared, &json!({"drive": 0, "titles": [3]}));
            assert_eq!(status, 400);
            assert_eq!(
                body["error"],
                json!(
                    crate::disc::DiscError::NoDestination.message(crate::i18n::Language::English)
                )
            );
            assert!(lock(&shared).queue.state.jobs.is_empty());
        }

        /// A disc folder is read through the same boundary as the file
        /// browser: a client cannot reach one outside the browse root.
        #[cfg(unix)]
        #[test]
        fn a_folder_outside_the_browse_root_is_refused() {
            use crate::disc::testing::{Fake, fake_makemkvcon};

            let root = std::env::temp_dir().join("av1c_api_disc_root");
            let outside = std::env::temp_dir().join("av1c_api_disc_outside");
            for dir in [&root, &outside] {
                let _ = std::fs::remove_dir_all(dir);
            }
            std::fs::create_dir_all(root.join("THE_DISC/BDMV")).unwrap();
            std::fs::create_dir_all(outside.join("THE_DISC/BDMV")).unwrap();

            let shared = scanned(Some("/tmp".to_string()));
            {
                let mut state = lock(&shared);
                state.config.daemon.browse_root = root.to_string_lossy().into_owned();
                let bin = fake_makemkvcon(&outside.join("bin"), &Fake::FolderScan);
                state.config.disc.makemkvcon_path = Some(bin.to_string_lossy().into_owned());
            }
            let (tx, _rx) = mpsc::channel();

            let (status, body) = discs_scan(
                &shared,
                &tx,
                &json!({"folder": outside.join("THE_DISC").to_string_lossy()}),
            );
            assert_eq!(status, 403);
            assert!(
                body["error"].as_str().unwrap().contains("browse root"),
                "{body}"
            );
            assert!(!lock(&shared).disc.active, "nothing was started");

            // The same folder under the root is scanned, and the scan records
            // the source the titles will belong to.
            let inside = root.join("THE_DISC");
            let (status, _) =
                discs_scan(&shared, &tx, &json!({"folder": inside.to_string_lossy()}));
            assert_eq!(status, 200);
            let state = lock(&shared);
            assert!(state.disc.active);
            assert_eq!(
                state.disc.scanned_source,
                Some(crate::disc::DiscSource::folder(inside.canonicalize().unwrap()).unwrap())
            );
            drop(state);

            for dir in [&root, &outside] {
                let _ = std::fs::remove_dir_all(dir);
            }
        }

        /// Once shutdown starts, neither a scan nor a rip claims the drive.
        #[cfg(unix)]
        #[test]
        fn no_disc_run_starts_once_shutdown_begins() {
            use crate::disc::testing::{Fake, fake_makemkvcon};

            let dir =
                std::env::temp_dir().join(format!("av1c_api_disc_shutdown_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let shared = scanned(Some("/tmp".to_string()));
            {
                let mut state = lock(&shared);
                let bin = fake_makemkvcon(&dir.join("bin"), &Fake::FolderScan);
                state.config.disc.makemkvcon_path = Some(bin.to_string_lossy().into_owned());
                state
                    .shutting_down
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            let (tx, _rx) = mpsc::channel();

            assert_eq!(discs_scan(&shared, &tx, &json!({"drive": 0})).0, 503);
            assert_eq!(rip(&shared, &json!({"drive": 0, "titles": [3]})).0, 503);
            let state = lock(&shared);
            assert!(!state.disc.active);
            assert!(state.queue.state.jobs.is_empty());
            drop(state);
            let _ = std::fs::remove_dir_all(dir);
        }

        /// The scan stores the canonical folder path; a rip request may spell
        /// the same folder through a symlink.
        #[cfg(unix)]
        #[test]
        fn a_symlinked_folder_rips_after_its_scan() {
            let root =
                std::env::temp_dir().join(format!("av1c_api_disc_link_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("THE_DISC/BDMV")).unwrap();
            std::os::unix::fs::symlink(root.join("THE_DISC"), root.join("LINK")).unwrap();

            let shared = scanned(Some("/tmp".to_string()));
            {
                let mut state = lock(&shared);
                state.config.daemon.browse_root = root.to_string_lossy().into_owned();
                state.disc.scanned_source = Some(
                    crate::disc::DiscSource::folder(root.join("THE_DISC").canonicalize().unwrap())
                        .unwrap(),
                );
            }

            let (_, body) = rip(
                &shared,
                &json!({"folder": root.join("LINK").to_string_lossy(), "titles": [3]}),
            );
            assert_ne!(body["error"], json!("scan the disc before ripping from it"));
            let _ = std::fs::remove_dir_all(&root);
        }

        /// A rip must be preceded by a scan of that same drive.
        #[test]
        fn ripping_an_unscanned_drive_is_refused() {
            let shared = scanned(Some("/tmp".to_string()));
            lock(&shared).disc.scanned_source = None;
            assert_eq!(rip(&shared, &json!({"drive": 0, "titles": [3]})).0, 400);
        }

        /// The dashboard poll carries disc state; there is no second poll.
        #[test]
        fn status_carries_the_disc_block() {
            let shared = scanned(Some("/tmp".to_string()));
            let value = status(&shared);
            assert_eq!(value["disc"]["active"], json!(false));
            assert_eq!(value["disc"]["drive"], json!(0));
            assert_eq!(value["disc"]["titles"][0]["id"], json!(3));
            assert_eq!(value["disc"]["titles"][0]["chapters"], json!(12));
            assert_eq!(value["disc"]["error"], Value::Null);
        }

        /// A drive listing is not reported as a running rip.
        #[test]
        fn a_drive_listing_is_not_reported_as_active() {
            let shared = scanned(Some("/tmp".to_string()));
            {
                let mut state = lock(&shared);
                state.disc.active = true;
                state.disc.listing = true;
            }
            assert_eq!(status(&shared)["disc"]["active"], json!(false));
        }

        /// The error and titles from the last run are dropped when a new
        /// dialog lists the drives.
        #[test]
        fn a_drive_listing_clears_the_last_error() {
            let shared = scanned(Some("/tmp".to_string()));
            {
                let mut state = lock(&shared);
                state.disc.error = Some("old failure".to_string());
                state.disc.disc_type = Some("Blu-ray disc".to_string());
                state.config.disc.makemkvcon_path = Some("/nonexistent/makemkvcon".to_string());
            }
            discs_list(&shared);
            let state = lock(&shared);
            assert_eq!(state.disc.error, None);
            assert!(state.disc.titles.is_empty());
            assert_eq!(state.disc.disc_type, None);
            assert_eq!(state.disc.scanned_source, None);
            assert!(!state.disc.active);
        }

        /// Cancelling raises the flag the run is watching.
        #[test]
        fn cancelling_reaches_the_running_worker() {
            let shared = scanned(Some("/tmp".to_string()));
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            {
                let mut state = lock(&shared);
                state.disc.active = true;
                state.disc.cancel_flag = Some(flag.clone());
            }
            assert_eq!(discs_cancel(&shared).0, 200);
            assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
            // The run stays active until its own event says it stopped.
            assert!(lock(&shared).disc.active);
        }
    }

    mod settings {
        use super::super::*;
        use crate::config::{DaemonConfig, DiscConfig};
        use crate::daemon::state::DaemonState;
        use std::sync::Mutex;

        fn guarded() -> DaemonConfig {
            DaemonConfig {
                auth_token: "s3cret".repeat(6),
                browse_root: "/media".to_string(),
                ..DaemonConfig::default()
            }
        }

        /// The live config a POST is merged onto.
        fn live() -> AppConfig {
            AppConfig {
                daemon: guarded(),
                disc: DiscConfig {
                    makemkvcon_path: Some("/opt/makemkvcon".to_string()),
                    ..DiscConfig::default()
                },
                ..AppConfig::default()
            }
        }

        /// The auth token is never included in a config response.
        #[test]
        fn the_auth_token_is_never_served() {
            let config = AppConfig {
                daemon: guarded(),
                ..AppConfig::default()
            };
            let value = redacted(&config);
            assert_eq!(value["daemon"]["auth_token"], json!(""));
            // Everything else is still reported as it stands.
            assert_eq!(value["daemon"]["browse_root"], json!("/media"));
        }

        /// A client cannot change `browse_root`, `auth_token`, or the path of
        /// the binary the server executes.
        #[test]
        fn the_daemon_block_survives_a_hostile_post() {
            let mut hostile = serde_json::to_value(AppConfig::default()).unwrap();
            hostile["daemon"]["browse_root"] = json!("");
            hostile["daemon"]["auth_token"] = json!("");
            hostile["daemon"]["bind_address"] = json!("0.0.0.0");
            hostile["disc"]["makemkvcon_path"] = json!("/tmp/evil.sh");

            let merged = merged_settings(&hostile, &live(), false).unwrap();
            assert_eq!(merged.daemon, guarded());
            assert_eq!(merged.disc, live().disc);
        }

        /// A save that moves the bind address back to loopback cannot clear
        /// `browse_root` while the running server still listens publicly.
        #[test]
        fn browse_root_cannot_be_cleared_while_bound_publicly() {
            let root = std::env::temp_dir().join(format!("av1c_bound_root_{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let live = AppConfig {
                daemon: DaemonConfig {
                    bind_address: "0.0.0.0".to_string(),
                    allow_insecure_lan: true,
                    browse_root: root.to_string_lossy().into_owned(),
                    ..DaemonConfig::default()
                },
                ..AppConfig::default()
            };
            let shared = Arc::new(Mutex::new(DaemonState::new(live.clone())));
            let mut body = serde_json::to_value(&live).unwrap();
            body["daemon"]["bind_address"] = json!("127.0.0.1");
            body["daemon"]["browse_root"] = json!("");

            assert_eq!(settings_post(&shared, &body, true).0, 400);
            assert_eq!(
                lock(&shared).config.daemon.browse_root,
                live.daemon.browse_root
            );
            let _ = std::fs::remove_dir_all(root);
        }

        /// Ordinary settings still apply, and are still sanitized on the way in.
        #[test]
        fn non_daemon_settings_still_apply() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["output"]["suffix"] = json!("../escape");

            let merged = merged_settings(&body, &AppConfig::default(), false).unwrap();
            assert_eq!(merged.output.suffix, "..escape");
        }

        #[test]
        fn out_of_range_numbers_are_rejected_instead_of_silently_clamped() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["audio"]["opus_bitrate_per_channel"] = json!(9000);
            assert!(merged_settings(&body, &AppConfig::default(), false).is_err());
        }

        #[test]
        fn invalid_nvenc_preset_is_rejected() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["performance"]["nvenc_preset"] = json!("slowest");
            assert!(merged_settings(&body, &AppConfig::default(), false).is_err());
        }

        /// Under a browse root, a missing directory outside it and an existing
        /// one outside it are refused with the same message.
        #[test]
        fn an_outside_output_directory_does_not_reveal_whether_it_exists() {
            let base =
                std::env::temp_dir().join(format!("av1c_settings_probe_{}", std::process::id()));
            let root = base.join("root");
            let outside = base.join("outside");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            let live = AppConfig {
                daemon: DaemonConfig {
                    browse_root: root.to_string_lossy().into_owned(),
                    ..DaemonConfig::default()
                },
                ..AppConfig::default()
            };
            let refusal = |directory: &Path| {
                let mut body = serde_json::to_value(&live).unwrap();
                body["output"]["same_directory"] = json!(false);
                body["output"]["output_directory"] = json!(directory);
                merged_settings(&body, &live, false).unwrap_err()
            };
            let expected =
                crate::i18n::t(live.language, Msg::OutputDirectoryOutsideBrowseRoot).to_string();
            assert_eq!(refusal(&outside), expected);
            assert_eq!(refusal(&base.join("missing")), expected);
            assert_eq!(refusal(&root.join("missing")), expected);
            let _ = std::fs::remove_dir_all(base);
        }

        #[test]
        fn output_directory_cannot_escape_browse_root() {
            let base =
                std::env::temp_dir().join(format!("av1c_settings_root_{}", std::process::id()));
            let root = base.join("root");
            let inside = root.join("output");
            let outside = base.join("outside");
            std::fs::create_dir_all(&inside).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            let daemon = DaemonConfig {
                browse_root: root.to_string_lossy().into_owned(),
                ..DaemonConfig::default()
            };
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["output"]["same_directory"] = json!(false);
            body["output"]["output_directory"] = json!(outside);
            assert!(
                merged_settings(
                    &body,
                    &AppConfig {
                        daemon: daemon.clone(),
                        ..AppConfig::default()
                    },
                    false,
                )
                .is_err()
            );

            // Ripped files use the output directory even when outputs sit
            // next to their sources.
            body["output"]["same_directory"] = json!(true);
            assert!(
                merged_settings(
                    &body,
                    &AppConfig {
                        daemon: daemon.clone(),
                        ..AppConfig::default()
                    },
                    false,
                )
                .is_err()
            );
            body["output"]["same_directory"] = json!(false);

            body["output"]["output_directory"] = json!(inside);
            let merged = merged_settings(
                &body,
                &AppConfig {
                    daemon: daemon.clone(),
                    ..AppConfig::default()
                },
                false,
            )
            .unwrap();
            assert_eq!(
                merged.output.output_directory,
                Some(
                    inside
                        .canonicalize()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                )
            );
            let _ = std::fs::remove_dir_all(base);
        }

        /// A finished job whose source is gone does not block a browse-root change.
        #[test]
        fn finished_jobs_do_not_block_a_browse_root_change() {
            use crate::daemon::state::DaemonState;
            use std::sync::{Arc, Mutex};

            let base =
                std::env::temp_dir().join(format!("av1c_settings_done_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            let old_root = base.join("old");
            let new_root = base.join("new");
            let output_dir = new_root.join("out");
            std::fs::create_dir_all(&old_root).unwrap();
            std::fs::create_dir_all(&output_dir).unwrap();

            let live = AppConfig {
                daemon: DaemonConfig {
                    browse_root: old_root.to_string_lossy().into_owned(),
                    ..DaemonConfig::default()
                },
                ..AppConfig::default()
            };
            let shared = Arc::new(Mutex::new(DaemonState::new(live.clone())));
            let mut job = EncodingJob::new(old_root.join("gone.mkv"));
            job.output_path = Some(old_root.join("gone.av1.mkv"));
            job.status = JobStatus::Done;
            lock(&shared).queue.push(job);

            let mut body = serde_json::to_value(&live).unwrap();
            body["daemon"]["browse_root"] = json!(new_root);
            body["output"]["same_directory"] = json!(false);
            body["output"]["output_directory"] = json!(output_dir);

            let (code, response) = settings_post(&shared, &body, true);
            assert_eq!(code, 200, "{response}");
            let _ = std::fs::remove_dir_all(base);
        }

        /// A settings body naming only some keys keeps the live values for the
        /// rest; a present key of the wrong type is refused.
        #[test]
        fn a_partial_settings_body_keeps_the_live_values() {
            let mut current = live();
            current.output.suffix = "_small".to_string();
            current.quality.delete_source_on_success = true;
            current.quality_preset = crate::config::QualityPreset::Custom;
            current.presets.sd.crf = 30;

            let merged = merged_settings(
                &json!({"quality": {"vmaf_threshold": 95.0}}),
                &current,
                true,
            )
            .unwrap();
            assert!((merged.quality.vmaf_threshold - 95.0).abs() < f64::EPSILON);
            assert!(merged.quality.delete_source_on_success);
            assert_eq!(merged.output.suffix, "_small");
            assert_eq!(merged.presets.sd.crf, 30);
            assert_eq!(merged.daemon.auth_token, current.daemon.auth_token);

            assert!(
                merged_settings(
                    &json!({"quality": {"vmaf_threshold": "95"}}),
                    &current,
                    true
                )
                .is_err()
            );
            assert!(merged_settings(&json!({"output": {"suffix": 5}}), &current, true).is_err());
        }

        /// A refused save that restores the live settings answers 409; one
        /// that cannot write them back answers 500.
        #[test]
        fn a_failed_settings_restore_is_a_server_error() {
            let current = live();
            let path = AppConfig::config_path();
            let (code, body) = restore_live_settings(&current, "conflict");
            assert_eq!(code, 409, "{body}");
            assert_eq!(body["error"], "conflict");
            assert_eq!(AppConfig::load_existing(), current);

            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir_all(path.join("blocker")).unwrap();
            let (code, body) = restore_live_settings(&current, "conflict");
            assert_eq!(code, 500, "{body}");
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|e| e
                        .starts_with(crate::i18n::t(current.language, Msg::SettingsRestoreFailed))),
                "{body}"
            );
            std::fs::remove_dir_all(&path).unwrap();
        }

        #[test]
        fn output_directory_is_required_when_outputs_are_separate() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["output"]["same_directory"] = json!(false);
            body["output"]["output_directory"] = Value::Null;
            assert!(merged_settings(&body, &AppConfig::default(), false).is_err());
        }

        /// A saved output directory that no longer exists does not block other
        /// changes and is reported back as a warning; a new directory must exist.
        #[test]
        fn an_unchanged_missing_output_directory_is_kept_with_a_warning() {
            use crate::daemon::state::DaemonState;
            use std::sync::{Arc, Mutex};

            let gone =
                std::env::temp_dir().join(format!("av1c_gone_output_{}", std::process::id()));
            let mut current = AppConfig::default();
            current.output.output_directory = Some(gone.to_string_lossy().into_owned());
            let mut body = serde_json::to_value(&current).unwrap();
            body["quality"]["vmaf_threshold"] = json!(91.0);

            let merged = merged_settings(&body, &current, false).unwrap();
            assert_eq!(
                merged.output.output_directory,
                current.output.output_directory
            );

            let shared = Arc::new(Mutex::new(DaemonState::new(current.clone())));
            let (code, response) = settings_post(&shared, &body, false);
            assert_eq!(code, 200, "{response}");
            assert!(response["_warning"].is_string(), "{response}");

            body["output"]["output_directory"] = json!(gone.join("other"));
            assert!(merged_settings(&body, &current, false).is_err());
        }

        #[test]
        fn local_settings_can_update_guarded_fields_without_exposing_the_token() {
            let current = live();
            let mut body = serde_json::to_value(&current).unwrap();
            body["daemon"]["enabled"] = json!(true);
            body["daemon"]["auth_token"] = json!("");
            let staging = std::env::temp_dir();
            body["disc"]["staging_directory"] = json!(staging);

            let merged = merged_settings(&body, &current, true).unwrap();

            assert!(merged.daemon.enabled);
            assert_eq!(merged.daemon.auth_token, current.daemon.auth_token);
            assert_eq!(
                merged.disc.staging_directory,
                Some(
                    staging
                        .canonicalize()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                )
            );
        }

        /// Whitespace around a new browse root is trimmed before the path is
        /// resolved.
        #[test]
        fn a_padded_browse_root_is_accepted() {
            let current = live();
            let root = std::env::temp_dir();
            let mut body = serde_json::to_value(&current).unwrap();
            body["daemon"]["auth_token"] = json!("");
            body["daemon"]["browse_root"] = json!(format!("  {}  ", root.display()));

            let merged = merged_settings(&body, &current, true).unwrap();

            assert_eq!(
                merged.daemon.browse_root,
                root.canonicalize().unwrap().to_string_lossy()
            );
        }

        #[test]
        fn local_token_replacement_requires_a_strong_value() {
            let current = live();
            let mut body = serde_json::to_value(&current).unwrap();
            body["daemon"]["auth_token"] = json!("too-short");
            assert!(merged_settings(&body, &current, true).is_err());

            let replacement = "0123456789abcdef0123456789abcdef";
            body["daemon"]["auth_token"] = json!(replacement);
            let merged = merged_settings(&body, &current, true).unwrap();
            assert_eq!(merged.daemon.auth_token, replacement);
        }

        /// The staging directory holds the queued rips, so it cannot be
        /// moved out from under them.
        #[test]
        fn changing_the_staging_directory_is_refused_while_rips_are_queued() {
            let mut live = live();
            live.disc.staging_directory = Some("/scratch".to_string());
            let mut moved = live.clone();
            moved.disc.staging_directory = Some("/other".to_string());
            let inside = |_: &Path, _: &str| true;

            let rip = [(PathBuf::from("/scratch/rip-a1/DISC_t00.mkv"), None, true)];
            assert_eq!(
                queue_conflict(&live, &moved, &rip, inside),
                Some(crate::i18n::t(
                    moved.language,
                    Msg::QueuedRipsPinStagingDirectory
                ))
            );
            let file = [(PathBuf::from("/media/movie.mkv"), None, false)];
            assert!(queue_conflict(&live, &moved, &file, inside).is_none());
            assert!(queue_conflict(&live, &live, &rip, inside).is_none());
        }

        /// The output directory cannot be cleared while ripped files wait to
        /// be encoded into it.
        #[test]
        fn clearing_the_output_directory_is_refused_while_rips_are_queued() {
            let mut live = live();
            live.output.output_directory = Some("/out".to_string());
            let mut cleared = live.clone();
            cleared.output.output_directory = None;
            let inside = |_: &Path, _: &str| true;

            let rip = [(PathBuf::from("/staging/rip-a1/DISC_t00.mkv"), None, true)];
            assert!(queue_conflict(&live, &cleared, &rip, inside).is_some());
            let file = [(PathBuf::from("/media/movie.mkv"), None, false)];
            assert!(queue_conflict(&live, &cleared, &file, inside).is_none());
            assert!(queue_conflict(&live, &live, &rip, inside).is_none());
        }

        /// A job queued since the settings snapshot is checked against the
        /// new browse root by resolved-path prefix.
        #[test]
        fn a_job_queued_since_the_snapshot_blocks_a_browse_root_change() {
            let live = live();
            let mut config = live.clone();
            config.daemon.browse_root = "/new-root".to_string();
            let prefix = |path: &Path, root: &str| path.starts_with(root);

            let outside = [(PathBuf::from("/media/movie.mkv"), None, false)];
            assert!(queue_conflict(&live, &config, &outside, prefix).is_some());
            let inside = [(PathBuf::from("/new-root/movie.mkv"), None, false)];
            assert!(queue_conflict(&live, &config, &inside, prefix).is_none());
            // An unchanged root is never a conflict.
            assert!(queue_conflict(&live, &live, &outside, prefix).is_none());
        }

        /// Paths confined against a browse root that has since changed are
        /// re-checked against the live one when they are queued.
        #[test]
        fn adding_rechecks_a_browse_root_changed_since_confinement() {
            use crate::daemon::state::DaemonState;
            use std::sync::{Arc, Mutex};

            let base = std::env::temp_dir().join(format!("av1c_add_root_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("root")).unwrap();
            let base = base.canonicalize().unwrap();
            let root = base.join("root");
            let outside = base.join("outside.mkv");
            let inside = root.join("inside.mkv");
            std::fs::write(&outside, b"video").unwrap();
            std::fs::write(&inside, b"video").unwrap();

            let config = AppConfig {
                daemon: DaemonConfig {
                    browse_root: root.to_string_lossy().into_owned(),
                    ..DaemonConfig::default()
                },
                ..AppConfig::default()
            };
            let shared = Arc::new(Mutex::new(DaemonState::new(config)));
            let (tx, _rx) = std::sync::mpsc::channel();

            crate::daemon::add_paths(&shared, &tx, vec![outside, inside.clone()], "");

            let state = lock(&shared);
            let queued: Vec<&PathBuf> = state.queue.state.jobs.iter().map(|j| &j.path).collect();
            assert_eq!(queued, [&inside]);
            drop(state);
            let _ = std::fs::remove_dir_all(base);
        }

        #[test]
        fn remote_clients_cannot_change_autostart() {
            let shared = Arc::new(std::sync::Mutex::new(
                crate::daemon::state::DaemonState::new(AppConfig::default()),
            ));
            let (status, _) = settings_service_post(&shared, &json!({"enabled": true}), false);
            assert_eq!(status, 403);
        }
    }

    #[test]
    fn only_one_recursive_scan_can_hold_the_server() {
        let shared = Arc::new(Mutex::new(DaemonState::new(AppConfig::default())));
        let first = RecursiveScanGuard::acquire(&shared).unwrap();
        assert!(RecursiveScanGuard::acquire(&shared).is_none());
        drop(first);
        assert!(RecursiveScanGuard::acquire(&shared).is_some());
    }

    mod tracks {
        use super::super::*;
        use crate::analyzer::VideoMetadata;
        use crate::daemon::state::DaemonState;
        use crate::queue::EncodingJob;
        use crate::tracks::AudioTrack;
        use std::sync::{Arc, Mutex};

        /// A queued job whose source carries Dolby Vision, on a chosen encoder.
        fn shared_with_dv_job(encoder: Encoder, dv_profile: Option<u8>) -> (SharedState, u64) {
            let mut state = DaemonState::new(AppConfig {
                encoder,
                ..AppConfig::default()
            });
            let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
            job.metadata = Some(VideoMetadata {
                width: 3840,
                height: 2160,
                hdr_type: HdrType::DolbyVision,
                dv_profile,
                dv_bl_compat: None,
                hdr10_static: None,
                codec_name: "hevc".to_string(),
                frame_rate_num: 24000,
                frame_rate_den: 1001,
                duration_secs: 60.0,
            });
            job.status = JobStatus::Ready;
            let id = state.queue.push(job);
            (Arc::new(Mutex::new(state)), id)
        }

        fn shared_with_job() -> (SharedState, u64) {
            let mut state = DaemonState::new(AppConfig::default());
            let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
            job.audio_tracks = audio_tracks(3);
            job.status = JobStatus::Ready;
            let id = state.queue.push(job);
            (Arc::new(Mutex::new(state)), id)
        }

        fn audio_tracks(count: usize) -> Vec<AudioTrack> {
            (0..count)
                .map(|index| AudioTrack {
                    index,
                    language: None,
                    codec: "dts".to_string(),
                    channels: Some(6),
                    channel_layout: Some("5.1(side)".to_string()),
                    title: None,
                    bitrate: None,
                    sample_rate: None,
                })
                .collect()
        }

        /// The track view reports what a `WebM` output forces: Opus for other
        /// audio codecs and no bitmap subtitles, while a remux keeps the source
        /// container.
        #[test]
        fn track_view_shows_what_the_container_forces() {
            let mut config = AppConfig::default();
            config.output.container = "webm".to_string();
            let mut state = DaemonState::new(config);
            let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
            job.audio_tracks = audio_tracks(1);
            job.subtitle_tracks = vec![crate::tracks::SubtitleTrack {
                index: 0,
                language: None,
                codec: "hdmv_pgs_subtitle".to_string(),
                title: None,
                forced: false,
            }];
            job.track_selection.audio_indices = vec![0];
            job.track_selection.subtitle_indices = vec![0];
            job.generate_output_path(&state.config.output);
            let id = state.queue.push(job);
            let shared = Arc::new(Mutex::new(state));

            let (code, body) = job_tracks(&shared, &id.to_string());
            assert_eq!(code, 200);
            assert_eq!(body["audio"][0]["opus_kbps"], json!(384));
            assert_eq!(
                body["audio"][0]["container_opus_kbps"],
                json!({"encode": 384, "remux": null})
            );
            assert_eq!(
                body["subtitles"][0]["container_drops"],
                json!({"encode": true, "remux": false})
            );
        }

        /// Opus indices are kept a subset of the selected tracks.
        #[test]
        fn opus_indices_are_confined_to_selected_tracks() {
            let (shared, id) = shared_with_job();

            let (code, body) = job_tracks_set(
                &shared,
                &json!({"id": id, "audio_indices": [0, 2], "audio_to_opus": [1, 2]}),
            );
            assert_eq!(code, 200);
            assert_eq!(body["audio_to_opus"], json!([2]));
            let state = lock(&shared);
            let selection = &state.queue.job_by_id(id).unwrap().track_selection;
            assert_eq!(selection.audio_indices, vec![0, 2]);
            assert_eq!(selection.audio_to_opus, vec![2]);
        }

        /// Indices the file does not have are rejected rather than stored.
        #[test]
        fn unknown_track_indices_are_rejected() {
            let (shared, id) = shared_with_job();
            let before = lock(&shared)
                .queue
                .job_by_id(id)
                .unwrap()
                .track_selection
                .clone();

            let (code, body) = job_tracks_set(
                &shared,
                &json!({"id": id, "audio_indices": [0, 99], "audio_to_opus": [99]}),
            );
            assert_eq!(code, 400);
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|e| e.contains("unknown track index")),
                "{body}"
            );
            let state = lock(&shared);
            let after = &state.queue.job_by_id(id).unwrap().track_selection;
            assert_eq!(after.audio_indices, before.audio_indices);
            assert!(!after.audio_indices.contains(&99));
        }

        /// A present key with the wrong type is refused and leaves the job as
        /// it was.
        #[test]
        fn a_track_field_of_the_wrong_type_is_refused() {
            let (shared, id) = shared_with_job();
            let before = lock(&shared).queue.job_by_id(id).unwrap().clone();

            for bad in [
                json!({"id": id, "audio_indices": "0"}),
                json!({"id": id, "subtitle_indices": {"0": true}}),
                json!({"id": id, "remux_only": "true"}),
                json!({"id": id, "audio_indices": [], "apply_to_remaining": 1}),
            ] {
                let (code, body) = job_tracks_set(&shared, &bad);
                assert_eq!(code, 400, "{bad} -> {body}");
            }
            let state = lock(&shared);
            let after = state.queue.job_by_id(id).unwrap();
            assert_eq!(
                after.track_selection.audio_indices,
                before.track_selection.audio_indices
            );
            assert_eq!(after.remux_only, before.remux_only);
        }

        #[test]
        fn saving_tracks_releases_job_for_encoding() {
            let (shared, id) = shared_with_job();
            lock(&shared).queue.job_by_id_mut(id).unwrap().status = JobStatus::AwaitingConfig;

            let (code, _) = job_tracks_set(&shared, &json!({"id": id}));

            assert_eq!(code, 200);
            assert!(matches!(
                lock(&shared).queue.job_by_id(id).unwrap().status,
                JobStatus::Ready
            ));
        }

        #[test]
        fn choices_apply_to_remaining_jobs_by_track_order() {
            let (shared, id) = shared_with_job();
            let remaining_id = {
                let mut state = lock(&shared);
                let mut job = EncodingJob::new(PathBuf::from("/tmp/episode-2.mkv"));
                job.audio_tracks = audio_tracks(4);
                job.track_selection.audio_indices = vec![0, 3];
                job.track_selection.audio_to_opus = vec![3];
                job.status = JobStatus::AwaitingConfig;
                state.queue.push(job)
            };

            assert_eq!(job_tracks(&shared, &id.to_string()).1["remaining"], 1);
            let (code, body) = job_tracks_set(
                &shared,
                &json!({
                    "id": id,
                    "audio_indices": [1],
                    "audio_to_opus": [1],
                    "remux_only": true,
                    "apply_to_remaining": true,
                }),
            );

            assert_eq!(code, 200);
            assert_eq!(body["applied"], 2);
            let state = lock(&shared);
            let remaining = state.queue.job_by_id(remaining_id).unwrap();
            assert_eq!(remaining.track_selection.audio_indices, vec![1, 3]);
            assert_eq!(remaining.track_selection.audio_to_opus, vec![1, 3]);
            assert!(!remaining.remux_only);
            assert!(matches!(remaining.status, JobStatus::Ready));
        }

        /// Each remaining job keeps the remux decision made for its own codec.
        #[test]
        fn apply_to_remaining_keeps_each_jobs_own_remux_flag() {
            let (shared, id) = shared_with_job();
            let av1_id = {
                let mut state = lock(&shared);
                state.queue.job_by_id_mut(id).unwrap().status = JobStatus::AwaitingConfig;
                let mut job = EncodingJob::new(PathBuf::from("/tmp/already-av1.mkv"));
                job.remux_only = true;
                job.status = JobStatus::AwaitingConfig;
                state.queue.push(job)
            };

            let (code, body) =
                job_tracks_set(&shared, &json!({"id": id, "apply_to_remaining": true}));

            assert_eq!(code, 200);
            assert_eq!(body["remux_only"], json!(false));
            let state = lock(&shared);
            let av1 = state.queue.job_by_id(av1_id).unwrap();
            assert!(av1.remux_only);
            assert!(matches!(av1.status, JobStatus::Ready));
        }

        /// A job that is already encoding refuses track edits.
        #[test]
        fn encoding_jobs_refuse_track_edits() {
            let (shared, id) = shared_with_job();
            lock(&shared).queue.job_by_id_mut(id).unwrap().status =
                JobStatus::Encoding { progress: 10.0 };

            let (code, _) = job_tracks_set(&shared, &json!({"id": id, "audio_indices": [0]}));
            assert_eq!(code, 409);
            assert_eq!(
                job_tracks(&shared, &id.to_string()).1["editable"],
                json!(false)
            );
        }

        /// The remux flag round-trips, and the output path is regenerated with
        /// it: a remux keeps the source container and its own suffix.
        #[test]
        fn remux_only_round_trips_and_renames_the_output() {
            let (shared, id) = shared_with_job();

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "remux_only": true}));
            assert_eq!(code, 200);
            assert_eq!(body["remux_only"], json!(true));

            let output = lock(&shared)
                .queue
                .job_by_id(id)
                .unwrap()
                .output_path
                .clone()
                .expect("remux job has an output path");
            assert_eq!(output.file_name().unwrap(), "movie_remux.mkv");
            assert_eq!(
                job_tracks(&shared, &id.to_string()).1["remux_only"],
                json!(true)
            );

            // And back again, onto the encode branch's suffix.
            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "remux_only": false}));
            assert_eq!(code, 200);
            assert_eq!(body["remux_only"], json!(false));
            let state = lock(&shared);
            let job = state.queue.job_by_id(id).unwrap();
            assert!(!job.remux_only);
            assert_eq!(
                job.output_path.as_ref().unwrap().file_name().unwrap(),
                "movie_av1.mkv"
            );
        }

        /// An absent `remux` field leaves the stored decision unchanged.
        #[test]
        fn omitting_remux_only_leaves_it_alone() {
            let (shared, id) = shared_with_job();
            lock(&shared).queue.job_by_id_mut(id).unwrap().remux_only = true;

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "audio_indices": [0]}));

            assert_eq!(code, 200);
            assert_eq!(body["remux_only"], json!(true));
            assert!(lock(&shared).queue.job_by_id(id).unwrap().remux_only);
        }

        /// Absent track lists leave the stored selection unchanged.
        #[test]
        fn omitting_track_lists_leaves_the_selection_alone() {
            let (shared, id) = shared_with_job();
            {
                let mut state = lock(&shared);
                let job = state.queue.job_by_id_mut(id).unwrap();
                job.subtitle_tracks = vec![crate::tracks::SubtitleTrack {
                    index: 3,
                    language: None,
                    codec: "subrip".to_string(),
                    title: None,
                    forced: false,
                }];
                job.track_selection.audio_indices = vec![0, 2];
                job.track_selection.audio_to_opus = vec![2];
                job.track_selection.subtitle_indices = vec![3];
            }

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "remux_only": true}));

            assert_eq!(code, 200);
            assert_eq!(body["audio_indices"], json!([0, 2]));
            assert_eq!(body["audio_to_opus"], json!([2]));
            assert_eq!(body["subtitle_indices"], json!([3]));
        }

        /// A source with no Dolby Vision layer has no DV decision to make.
        #[test]
        fn dv_mode_is_rejected_for_a_non_dv_job() {
            let (shared, id) = shared_with_job();

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "dv_mode": "keep"}));

            assert_eq!(code, 400);
            assert!(
                body["error"].as_str().unwrap().contains("Dolby Vision"),
                "unexpected error: {}",
                body["error"]
            );
            assert!(lock(&shared).queue.job_by_id(id).unwrap().dv_mode.is_none());
        }

        /// Keeping DV is refused on any encoder but SVT-AV1, which is the only
        /// one that can write the RPU.
        #[test]
        fn keeping_dv_is_refused_when_the_encoder_cannot_write_the_rpu() {
            let (shared, id) = shared_with_dv_job(Encoder::Nvenc, Some(8));

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "dv_mode": "keep"}));
            assert_eq!(code, 400);
            assert!(
                body["error"].as_str().unwrap().contains("RPU"),
                "unexpected error: {}",
                body["error"]
            );

            // The UI is told the same up front.
            let offered = job_tracks(&shared, &id.to_string()).1;
            assert_eq!(offered["dv"]["can_keep"], json!(false));
            assert_eq!(offered["dv"]["mode"], json!("hdr10"));
            assert_eq!(offered["dv"]["profile"], json!(8));

            // Converting to HDR10 is still allowed on that encoder.
            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "dv_mode": "hdr10"}));
            assert_eq!(code, 200);
            assert_eq!(body["dv_mode"], json!("hdr10"));
        }

        /// A keep mode chosen under SVT-AV1 reads and saves as HDR10 after a
        /// switch to a hardware encoder.
        #[test]
        fn a_stored_keep_mode_saves_as_hdr10_on_a_hardware_encoder() {
            let (shared, id) = shared_with_dv_job(Encoder::Nvenc, Some(8));
            lock(&shared).queue.job_by_id_mut(id).unwrap().dv_mode = Some(DvMode::KeepDolbyVision);

            let offered = job_tracks(&shared, &id.to_string()).1;
            assert_eq!(offered["dv"]["mode"], json!("hdr10"));

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "dv_mode": "keep"}));
            assert_eq!(code, 200, "{body}");
            assert_eq!(body["dv_mode"], json!("hdr10"));
        }

        /// On SVT-AV1 the choice is real and round-trips.
        #[test]
        fn dv_mode_round_trips_on_svt_av1() {
            let (shared, id) = shared_with_dv_job(Encoder::SvtAv1, Some(7));

            let offered = job_tracks(&shared, &id.to_string()).1;
            assert_eq!(offered["dv"]["can_keep"], json!(true));
            assert_eq!(offered["dv"]["recommended"], json!("keep"));
            let (profile5, profile5_id) = shared_with_dv_job(Encoder::SvtAv1, Some(5));
            assert_eq!(
                job_tracks(&profile5, &profile5_id.to_string()).1["dv"]["recommended"],
                json!("hdr10")
            );

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "dv_mode": "keep"}));
            assert_eq!(code, 200);
            assert_eq!(body["dv_mode"], json!("keep"));
            assert_eq!(
                lock(&shared).queue.job_by_id(id).unwrap().dv_mode,
                Some(DvMode::KeepDolbyVision)
            );

            let (code, _) = job_tracks_set(&shared, &json!({"id": id, "dv_mode": "nonsense"}));
            assert_eq!(code, 400);
        }

        /// A job with no DV layer reports no DV block at all.
        #[test]
        fn a_non_dv_job_offers_no_dv_choice() {
            let (shared, id) = shared_with_job();
            assert_eq!(job_tracks(&shared, &id.to_string()).1["dv"], json!(null));
        }

        /// Turning remux off on a DV source resolves the DV mode.
        #[test]
        fn leaving_remux_resolves_a_pending_dv_decision() {
            let (shared, id) = shared_with_dv_job(Encoder::SvtAv1, Some(5));
            lock(&shared).queue.job_by_id_mut(id).unwrap().remux_only = true;

            let (code, body) = job_tracks_set(&shared, &json!({"id": id, "remux_only": false}));

            assert_eq!(code, 200);
            // Profile 5 has no HDR10-compatible base layer.
            assert_eq!(body["dv_mode"], json!("hdr10"));
        }

        /// The encoding guard covers the per-job options, not just tracks.
        #[test]
        fn encoding_jobs_refuse_option_edits() {
            let (shared, id) = shared_with_dv_job(Encoder::SvtAv1, Some(8));
            lock(&shared).queue.job_by_id_mut(id).unwrap().status =
                JobStatus::Encoding { progress: 10.0 };

            let (code, _) = job_tracks_set(
                &shared,
                &json!({"id": id, "remux_only": true, "dv_mode": "keep"}),
            );

            assert_eq!(code, 409);
            let state = lock(&shared);
            let job = state.queue.job_by_id(id).unwrap();
            assert!(!job.remux_only);
            assert!(job.dv_mode.is_none());
        }
    }
}
