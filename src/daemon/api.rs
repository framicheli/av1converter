use super::state::{SharedState, is_terminal, lock};
use crate::analyzer::{DvMode, HdrType};
use crate::config::{AppConfig, AudioConfig, Encoder, EncodingPreset};
use crate::disc::worker::DiscEvent;
use crate::queue::{
    EncodingJob, JobStatus, collect_video_files, collect_video_files_within, is_video_file,
    make_output_paths_unique,
};
use crate::tracks::TrackSelection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

/// Dashboard poll: overall daemon and encode-session status.
pub fn status(shared: &SharedState) -> Value {
    let state = lock(shared);
    let queue = &state.queue.state;

    let current = queue
        .jobs
        .get(queue.current_job_index)
        .filter(|job| {
            matches!(
                job.status,
                JobStatus::Encoding { .. } | JobStatus::Verifying
            )
        })
        .map(|job| {
            json!({
                "id": state.queue.ids().get(queue.current_job_index),
                "filename": job.filename(),
                "status": status_json(&job.status),
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
            "skipped": queue.skipped_count,
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

    json!({
        "active": disc.active,
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
        .map(|(id, job)| {
            let saved_percent = job.size_reduction().map(|(_, percent)| percent);
            json!({
                "id": id,
                "filename": job.filename(),
                "path": job.path.to_string_lossy(),
                "output_path": job.output_path.as_ref().map(|p| p.to_string_lossy()),
                "status": status_json(&job.status),
                "resolution": job.resolution_string(),
                "hdr": job.hdr_string(),
                "remux_only": job.remux_only,
                "source_size": job.source_size,
                "output_size": job.output_size,
                "saved_percent": saved_percent,
                "source_deleted": job.source_deleted,
            })
        })
        .collect();
    json!({ "jobs": jobs })
}

/// Whether a job's tracks can still be changed. False once its encode is under
/// way: the selection is baked into the running `FFmpeg` command.
fn tracks_editable(state: &super::state::DaemonState, id: u64) -> bool {
    !state.in_active_session(id)
        && state
            .queue
            .job_by_id(id)
            .is_some_and(|job| matches!(job.status, JobStatus::Ready | JobStatus::AwaitingConfig))
}

/// The DV mode a job falls back to when nobody has chosen one: the profile's
/// own recommendation on SVT-AV1, HDR10 on every other encoder.
pub fn resolved_dv_mode(encoder: Encoder, dv_profile: Option<u8>) -> DvMode {
    if encoder == Encoder::SvtAv1 {
        DvMode::recommended_for(dv_profile)
    } else {
        DvMode::ToHdr10
    }
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

    // Resolved so the row shows the bitrate the encoder is actually asked for,
    // including already-Opus tracks, which are left alone.
    let plan = job
        .track_selection
        .resolve(&job.audio_tracks, &audio_config);
    let audio: Vec<Value> = job
        .audio_tracks
        .iter()
        .map(|track| {
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
            })
        })
        .collect();

    let subtitles: Vec<Value> = job
        .subtitle_tracks
        .iter()
        .map(|track| {
            json!({
                "index": track.index,
                "name": track.display_name(),
                "selected": job.track_selection.subtitle_indices.contains(&track.index),
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
            let effective = job
                .dv_mode
                .unwrap_or_else(|| resolved_dv_mode(encoder, meta.dv_profile));
            json!({
                "profile": meta.dv_profile,
                "mode": dv_mode_name(effective),
                "can_keep": encoder == Encoder::SvtAv1,
            })
        });

    (
        200,
        json!({
            "id": id,
            "filename": job.filename(),
            "editable": tracks_editable(&state, id),
            "audio": audio,
            "subtitles": subtitles,
            "remux_only": job.remux_only,
            "dv": dv,
            "remaining": remaining,
        }),
    )
}

/// Read a JSON array of track indices, keeping only ones the job really has.
fn valid_indices(body: &Value, key: &str, known: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = body
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_u64)
                .filter_map(|i| usize::try_from(i).ok())
                .filter(|i| known.contains(i))
                .collect()
        })
        .unwrap_or_default();
    out.sort_unstable();
    out.dedup();
    out
}

/// Map one file's choices onto another file by track order. Extra target
/// tracks keep their automatic selection instead of being silently dropped.
// Order mapping targets same-layout batches; matching language/title is the
// upgrade for mixed-layout ones.
fn mapped_selection(
    job: &EncodingJob,
    audio_modes: &[Option<bool>],
    subtitle_selected: &[bool],
) -> TrackSelection {
    let audio_indices = job
        .audio_tracks
        .iter()
        .enumerate()
        .filter(|(position, track)| {
            audio_modes.get(*position).map_or_else(
                || job.track_selection.audio_indices.contains(&track.index),
                Option::is_some,
            )
        })
        .map(|(_, track)| track.index)
        .collect();
    let audio_to_opus = job
        .audio_tracks
        .iter()
        .enumerate()
        .filter(|(position, track)| {
            audio_modes.get(*position).map_or_else(
                || job.track_selection.is_opus(track.index),
                |mode| *mode == Some(true),
            )
        })
        .map(|(_, track)| track.index)
        .collect();
    let subtitle_indices = job
        .subtitle_tracks
        .iter()
        .enumerate()
        .filter(|(position, track)| {
            subtitle_selected
                .get(*position)
                .copied()
                .unwrap_or_else(|| job.track_selection.subtitle_indices.contains(&track.index))
        })
        .map(|(_, track)| track.index)
        .collect();
    TrackSelection {
        audio_indices,
        subtitle_indices,
        audio_to_opus,
    }
}

fn apply_track_config(
    job: &mut EncodingJob,
    selection: TrackSelection,
    remux_only: bool,
    dv_mode: Option<DvMode>,
    output: &crate::config::OutputConfig,
    encoder: Encoder,
) {
    job.track_selection = selection;
    job.remux_only = remux_only;
    job.dv_mode = dv_mode;
    if !remux_only
        && job.dv_mode.is_none()
        && let Some(profile) = job
            .metadata
            .as_ref()
            .filter(|meta| meta.hdr_type == HdrType::DolbyVision)
            .map(|meta| meta.dv_profile)
    {
        job.dv_mode = Some(resolved_dv_mode(encoder, profile));
    }
    job.generate_output_path(output);
    job.status = JobStatus::Ready;
}

/// Replace one job's track selection and per-job options.
#[allow(clippy::too_many_lines)]
pub fn job_tracks_set(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };

    let mut state = lock(shared);
    let encoder = state.config.encoder;
    let output_config = state.config.output.clone();

    let (selection, remux_only, dv_mode, audio_modes, subtitle_selected) = {
        let Some(job) = state.queue.job_by_id(id) else {
            return (404, json!({"error": "unknown job id"}));
        };
        if !tracks_editable(&state, id) {
            return (409, json!({"error": "job is encoding or already finished"}));
        }

        let audio_known: Vec<usize> = job.audio_tracks.iter().map(|t| t.index).collect();
        let subtitle_known: Vec<usize> = job.subtitle_tracks.iter().map(|t| t.index).collect();

        let audio_indices = valid_indices(body, "audio_indices", &audio_known);
        // Kept a subset of the selection: the plan indexes per-stream codec
        // options by output position.
        let audio_to_opus: Vec<usize> = valid_indices(body, "audio_to_opus", &audio_known)
            .into_iter()
            .filter(|i| audio_indices.contains(i))
            .collect();

        let selection = crate::tracks::TrackSelection {
            audio_indices,
            subtitle_indices: valid_indices(body, "subtitle_indices", &subtitle_known),
            audio_to_opus,
        };

        // Both options are absent-means-unchanged, so a client that only knows
        // about tracks leaves them as they stand.
        let remux_only = body
            .get("remux_only")
            .and_then(Value::as_bool)
            .unwrap_or(job.remux_only);

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
    apply_track_config(job, selection, remux_only, dv_mode, &output_config, encoder);

    let mut applied = json!({
        "audio_indices": job.track_selection.audio_indices,
        "audio_to_opus": job.track_selection.audio_to_opus,
        "subtitle_indices": job.track_selection.subtitle_indices,
        "remux_only": job.remux_only,
        "dv_mode": job.dv_mode.map(dv_mode_name),
    });

    let mut applied_count = 1;
    if body
        .get("apply_to_remaining")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        for job in &mut state.queue.state.jobs {
            if !matches!(job.status, JobStatus::AwaitingConfig) {
                continue;
            }
            let selection = mapped_selection(job, &audio_modes, &subtitle_selected);
            let target_dv = job
                .metadata
                .as_ref()
                .is_some_and(|meta| meta.hdr_type == HdrType::DolbyVision)
                .then(|| dv_mode.or(job.dv_mode))
                .flatten();
            apply_track_config(
                job,
                selection,
                remux_only,
                target_dv,
                &output_config,
                encoder,
            );
            applied_count += 1;
        }
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

/// Expand an add request into concrete video files and queue them.
pub fn queue_add(
    shared: &SharedState,
    probe_tx: &Sender<(u64, String)>,
    body: &Value,
) -> (u16, Value) {
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
    if !path.exists() {
        return (400, json!({"error": "path does not exist"}));
    }
    let browse_root = lock(shared).config.daemon.browse_root.clone();
    let Some(path) = confined_path(&path, &browse_root) else {
        return (
            403,
            json!({"error": "path is outside the configured browse root"}),
        );
    };

    let mut files: Vec<PathBuf> = Vec::new();
    match mode {
        // An explicitly chosen file is queued as asked, even when it looks like
        // one of our own outputs.
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
            if let Ok(entries) = std::fs::read_dir(&path) {
                files.extend(
                    entries
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .filter(|p| is_video_file(p)),
                );
            }
        }
        "folder_recursive" => {
            if !path.is_dir() {
                return (400, json!({"error": "not a directory"}));
            }
            if browse_root.is_empty() {
                collect_video_files(&path, &mut files);
            } else {
                collect_video_files_within(&path, Path::new(&browse_root), &mut files);
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
        return (400, json!({"error": "no video files found"}));
    }
    files.sort();

    let (added, already_queued) = super::add_paths(shared, probe_tx, files);
    (
        200,
        json!({"added": added, "already_queued": already_queued}),
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

/// Remove a job unless it belongs to the running encode session.
pub fn queue_remove(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };
    let mut state = lock(shared);
    if state.queue.job_by_id(id).is_none() {
        return (404, json!({"error": "unknown job id"}));
    }
    if state.in_active_session(id) {
        return (
            409,
            json!({"error": "job is part of the active encode session"}),
        );
    }
    state.queue.remove(id);
    (200, json!({"ok": true}))
}

/// Cancel the running encode session.
pub fn queue_cancel(shared: &SharedState) -> (u16, Value) {
    let state = lock(shared);
    if let Some(session) = state.session.as_ref().filter(|_| state.encoding_active) {
        session
            .cancel_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    (200, json!({"ok": true}))
}

/// Drop all jobs in terminal states.
pub fn queue_clear_finished(shared: &SharedState) -> (u16, Value) {
    let mut state = lock(shared);
    let finished: Vec<u64> = state
        .queue
        .jobs_with_ids()
        .filter(|(_, job)| is_terminal(&job.status))
        .map(|(id, _)| id)
        .collect();
    let removed = finished.len();
    for id in finished {
        state.queue.remove(id);
    }
    (200, json!({"removed": removed}))
}

/// Server-side file browser: list one directory level.
pub fn fs_browse(shared: &SharedState, path: &str, show_hidden: bool) -> (u16, Value) {
    let browse_root = lock(shared).config.daemon.browse_root.clone();
    let requested = if path.is_empty() {
        // With a root configured, that is where browsing starts.
        if browse_root.is_empty() {
            std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
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
    let (config, busy) = {
        let state = lock(shared);
        (state.config.clone(), state.disc.active)
    };
    if busy {
        return (409, json!({"error": "a disc operation is already running"}));
    }

    let bin = match crate::disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => return disc_failure(&e, config.language),
    };
    // Listing takes a second or two and holds no lock.
    let cancel = std::sync::atomic::AtomicBool::new(false);
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

/// Start scanning a drive (`{"drive": N}`) or a disc folder on disk
/// (`{"folder": "<path>"}`). The titles arrive in `/api/status`.
pub fn discs_scan(shared: &SharedState, disc_tx: &Sender<DiscEvent>, body: &Value) -> (u16, Value) {
    let mut state = lock(shared);
    if state.disc.active {
        return (409, json!({"error": "a disc operation is already running"}));
    }
    let config = state.config.clone();

    let source = if let Some(folder) = body.get("folder").and_then(Value::as_str) {
        // The same boundary the file browser enforces: a client cannot read a
        // disc from outside the browse root.
        let Some(path) = confined_path(Path::new(folder), &config.daemon.browse_root) else {
            return (
                400,
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
        let Some(disc_drive) = state
            .disc
            .drives
            .iter()
            .find(|known| known.id == drive)
            .cloned()
        else {
            return (400, json!({"error": "unknown drive id"}));
        };
        crate::disc::DiscSource::Drive(disc_drive)
    };

    let bin = match crate::disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => return disc_failure(&e, config.language),
    };

    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.disc.cancel_flag = Some(cancel.clone());
    state.disc.active = true;
    state.disc.scanning = true;
    state.disc.error = None;
    state.disc.titles.clear();
    state.disc.scanned_source = Some(source.clone());
    drop(state);

    crate::disc::worker::spawn_scan(bin, source, &cancel, disc_tx.clone());
    (200, json!({"ok": true}))
}

/// Extract the named titles, one after another, into the staging directory.
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

    let mut state = lock(shared);
    if state.disc.active {
        return (409, json!({"error": "a disc operation is already running"}));
    }
    let Some(source) = state
        .disc
        .scanned_source
        .clone()
        .filter(|scanned| match scanned {
            crate::disc::DiscSource::Drive(drive) => {
                body.get("drive").and_then(Value::as_u64).and_then(as_id) == Some(drive.id)
            }
            crate::disc::DiscSource::Folder(path) => {
                body.get("folder").and_then(Value::as_str) == path.to_str()
            }
        })
    else {
        return (
            400,
            json!({"error": "scan the disc before ripping from it"}),
        );
    };
    // Only titles this server reported, and each of them once.
    let mut titles = Vec::new();
    for id in &ids {
        let Some(title) = state.disc.titles.iter().find(|title| title.id == *id) else {
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
    let config = state.config.clone();
    if let Err(e) = crate::disc::staging::require_destination(&config) {
        return disc_failure(&e, config.language);
    }
    let bin = match crate::disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => return disc_failure(&e, config.language),
    };

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
    state.disc.cancel_flag = Some(cancel.clone());
    state.disc.active = true;
    state.disc.scanning = false;
    state.disc.error = None;
    state.disc.job_ids.clone_from(&job_ids);
    drop(state);

    crate::disc::worker::spawn_rips(bin, config, source, titles, &cancel, disc_tx.clone());
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
        _ => 500,
    };
    (status, json!({"error": error.message(lang)}))
}

/// Read the full configuration, minus the auth token.
pub fn settings_get(shared: &SharedState) -> Value {
    redacted(&lock(shared).config)
}

/// Read a client-supplied configuration, keeping the live `[daemon]` and
/// `[disc]` blocks. `browse_root`, `auth_token`, bind address, port and the
/// path to `makemkvcon` stay editable from the config file and the TUI only.
fn merged_settings(body: &Value, live: &AppConfig) -> Result<AppConfig, String> {
    let mut config: AppConfig =
        serde_json::from_value(body.clone()).map_err(|e| format!("invalid settings: {e}"))?;
    config.daemon = live.daemon.clone();
    config.disc = live.disc.clone();
    validate_numeric_settings(&config)?;
    if let Some(presets) = config.quality_preset.presets() {
        config.presets = presets;
    }
    config.sanitize();
    if !config.output.same_directory {
        let directory = config
            .output
            .output_directory
            .as_deref()
            .filter(|dir| !dir.is_empty())
            .ok_or_else(|| "output directory is required".to_string())?;
        let path = Path::new(directory);
        if !path.is_dir() {
            return Err("output directory does not exist".to_string());
        }
        let Some(path) = confined_path(path, &config.daemon.browse_root) else {
            return Err("output directory must be inside browse_root".to_string());
        };
        config.output.output_directory = Some(path.to_string_lossy().into_owned());
    }
    Ok(config)
}

fn validate_numeric_settings(config: &AppConfig) -> Result<(), String> {
    if !config.quality.vmaf_threshold.is_finite()
        || !(0.0..=100.0).contains(&config.quality.vmaf_threshold)
    {
        return Err("VMAF threshold must be between 0 and 100".to_string());
    }
    if config.performance.svt_preset > 13 {
        return Err("SVT preset must be between 0 and 13".to_string());
    }
    if !crate::config::PerformanceConfig::valid_nvenc_preset(&config.performance.nvenc_preset) {
        return Err("NVENC preset must be between p1 and p7".to_string());
    }
    if !(AudioConfig::MIN_PER_CHANNEL..=AudioConfig::MAX_PER_CHANNEL)
        .contains(&config.audio.opus_bitrate_per_channel)
    {
        return Err("Opus bitrate per channel must be between 16 and 256".to_string());
    }
    let presets: [&EncodingPreset; 8] = [
        &config.presets.sd,
        &config.presets.hd,
        &config.presets.full_hd,
        &config.presets.full_hd_hdr,
        &config.presets.full_hd_dv,
        &config.presets.uhd,
        &config.presets.uhd_hdr,
        &config.presets.uhd_dv,
    ];
    if presets.iter().any(|preset| {
        preset.crf > Encoder::SvtAv1.max_quality()
            || preset.nvenc_cq > Encoder::Nvenc.max_quality()
            || preset.qsv_quality > Encoder::Qsv.max_quality()
            || preset.amf_quality > Encoder::Amf.max_quality()
            || preset.film_grain > 50
    }) {
        return Err("one or more rate-factor or film-grain values are out of range".to_string());
    }
    Ok(())
}

/// Replace the configuration: sanitize, persist to config.toml, and swap the
/// live copy. Changes apply from the next analysis/encode.
pub fn settings_post(shared: &SharedState, body: &Value) -> (u16, Value) {
    let mut state = lock(shared);
    let config = match merged_settings(body, &state.config) {
        Ok(config) => config,
        Err(e) => return (400, json!({"error": e})),
    };
    if let Err(e) = config.save() {
        return (500, json!({"error": format!("failed to save: {e}")}));
    }
    let output_changed = state.config.output != config.output;
    let saved = redacted(&config);
    state.config = config;
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
        JobStatus::QualityWarning { vmaf, threshold } => {
            json!({"kind": "quality_warning", "vmaf": vmaf, "threshold": threshold})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RecursiveScanGuard, queue_add, within_root};
    use crate::config::{AppConfig, DaemonConfig};
    use crate::daemon::state::{DaemonState, lock};
    use std::path::Path;
    use std::sync::{Arc, Mutex};

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
            assert_eq!(status, 400);
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

        fn guarded() -> DaemonConfig {
            DaemonConfig {
                auth_token: "s3cret".to_string(),
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

            let merged = merged_settings(&hostile, &live()).unwrap();
            assert_eq!(merged.daemon, guarded());
            assert_eq!(merged.disc, live().disc);
        }

        /// Ordinary settings still apply, and are still sanitized on the way in.
        #[test]
        fn non_daemon_settings_still_apply() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["output"]["suffix"] = json!("../escape");

            let merged = merged_settings(&body, &AppConfig::default()).unwrap();
            assert_eq!(merged.output.suffix, "..escape");
        }

        #[test]
        fn out_of_range_numbers_are_rejected_instead_of_silently_clamped() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["audio"]["opus_bitrate_per_channel"] = json!(9000);
            assert!(merged_settings(&body, &AppConfig::default()).is_err());
        }

        #[test]
        fn invalid_nvenc_preset_is_rejected() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["performance"]["nvenc_preset"] = json!("slowest");
            assert!(merged_settings(&body, &AppConfig::default()).is_err());
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
                    }
                )
                .is_err()
            );

            body["output"]["output_directory"] = json!(inside);
            let merged = merged_settings(
                &body,
                &AppConfig {
                    daemon: daemon.clone(),
                    ..AppConfig::default()
                },
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

        #[test]
        fn output_directory_is_required_when_outputs_are_separate() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["output"]["same_directory"] = json!(false);
            body["output"]["output_directory"] = Value::Null;
            assert!(merged_settings(&body, &AppConfig::default()).is_err());
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

        /// Indices the file does not have are dropped rather than stored.
        #[test]
        fn unknown_track_indices_are_rejected() {
            let (shared, id) = shared_with_job();

            let (code, body) = job_tracks_set(
                &shared,
                &json!({"id": id, "audio_indices": [0, 99], "audio_to_opus": [99]}),
            );
            assert_eq!(code, 200);
            assert_eq!(body["audio_indices"], json!([0]));
            assert_eq!(body["audio_to_opus"], json!([]));
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
            assert!(remaining.remux_only);
            assert!(matches!(remaining.status, JobStatus::Ready));
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

        /// On SVT-AV1 the choice is real and round-trips.
        #[test]
        fn dv_mode_round_trips_on_svt_av1() {
            let (shared, id) = shared_with_dv_job(Encoder::SvtAv1, Some(7));

            let offered = job_tracks(&shared, &id.to_string()).1;
            assert_eq!(offered["dv"]["can_keep"], json!(true));

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
