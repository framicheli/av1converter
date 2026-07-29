use super::state::{SharedState, is_terminal, lock};
use crate::config::{AppConfig, DaemonConfig};
use crate::queue::{JobStatus, collect_video_files, collect_video_files_within, is_video_file};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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
    let (saved_bytes, saved_human) = queue.total_space_saved();

    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "encoder": state.config.encoder.display_name(),
        "encoding_active": state.encoding_active,
        "paused": state.paused,
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "overall_progress": queue.overall_progress(),
        "eta_secs": queue.estimated_time_remaining().map(|d| d.as_secs()),
        "elapsed_secs": queue.elapsed_time().map(|d| d.as_secs()),
        "counts": {
            "total": queue.jobs.len(),
            "ready": ready,
            "converted": queue.converted_count,
            "skipped": queue.skipped_count,
            "errors": queue.error_count,
        },
        "total_space_saved": { "bytes": saved_bytes, "human": saved_human },
        "current": current,
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

/// Whether a job's tracks can still be changed: once its encode is under way,
/// the selection is already baked into the running `FFmpeg` command.
fn tracks_editable(state: &super::state::DaemonState, id: u64) -> bool {
    !state.in_active_session(id)
        && state
            .queue
            .job_by_id(id)
            .is_some_and(|job| matches!(job.status, JobStatus::Ready | JobStatus::AwaitingConfig))
}

/// One job's audio and subtitle tracks, with the current per-track choices.
pub fn job_tracks(shared: &SharedState, id_param: &str) -> (u16, Value) {
    let Ok(id) = id_param.parse::<u64>() else {
        return (400, json!({"error": "missing or invalid 'id'"}));
    };
    let state = lock(shared);
    let audio_config = state.config.audio.clone();
    let Some(job) = state.queue.job_by_id(id) else {
        return (404, json!({"error": "unknown job id"}));
    };

    // Resolving here means the row shows the bitrate the encoder will actually
    // be asked for, including tracks that are already Opus and so left alone.
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

    (
        200,
        json!({
            "id": id,
            "filename": job.filename(),
            "editable": tracks_editable(&state, id),
            "audio": audio,
            "subtitles": subtitles,
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

/// Replace one job's track selection.
pub fn job_tracks_set(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };

    let mut state = lock(shared);
    let selection = {
        let Some(job) = state.queue.job_by_id(id) else {
            return (404, json!({"error": "unknown job id"}));
        };
        if !tracks_editable(&state, id) {
            return (409, json!({"error": "job is encoding or already finished"}));
        }

        let audio_known: Vec<usize> = job.audio_tracks.iter().map(|t| t.index).collect();
        let subtitle_known: Vec<usize> = job.subtitle_tracks.iter().map(|t| t.index).collect();

        let audio_indices = valid_indices(body, "audio_indices", &audio_known);
        // Opus is only meaningful for tracks that are actually written, and a
        // stale entry would shift every per-stream codec option one place.
        let audio_to_opus: Vec<usize> = valid_indices(body, "audio_to_opus", &audio_known)
            .into_iter()
            .filter(|i| audio_indices.contains(i))
            .collect();

        crate::tracks::TrackSelection {
            audio_indices,
            subtitle_indices: valid_indices(body, "subtitle_indices", &subtitle_known),
            audio_to_opus,
        }
    };

    let applied = json!({
        "audio_indices": selection.audio_indices,
        "audio_to_opus": selection.audio_to_opus,
        "subtitle_indices": selection.subtitle_indices,
    });
    state
        .queue
        .job_by_id_mut(id)
        .expect("job checked above")
        .track_selection = selection;
    (200, applied)
}

/// Whether `path` sits inside `root`, comparing resolved paths so that `..`
/// segments and symlinks cannot step outside. An empty root allows everything.
pub fn within_root(path: &Path, root: &str) -> bool {
    if root.is_empty() {
        return true;
    }
    let (Ok(path), Ok(root)) = (path.canonicalize(), PathBuf::from(root).canonicalize()) else {
        return false;
    };
    path.starts_with(root)
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
    let path = PathBuf::from(path);
    if !path.exists() {
        return (400, json!({"error": "path does not exist"}));
    }
    let browse_root = lock(shared).config.daemon.browse_root.clone();
    if !within_root(&path, &browse_root) {
        return (
            403,
            json!({"error": "path is outside the configured browse root"}),
        );
    }

    let mut files: Vec<PathBuf> = Vec::new();
    match mode {
        // An explicitly chosen file is queued as asked, even if it looks like
        // one of our own outputs — the user pointed straight at it.
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

    // Symlinks are followed while scanning, so a link can lead back out of the
    // browse root even when the folder given was inside it.
    files.retain(|p| within_root(p, &browse_root));

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

/// Pause/resume auto-starting new sessions (does not stop the current one).
pub fn queue_pause(shared: &SharedState, body: &Value) -> (u16, Value) {
    let Some(paused) = body.get("paused").and_then(Value::as_bool) else {
        return (400, json!({"error": "missing 'paused'"}));
    };
    lock(shared).paused = paused;
    (200, json!({"paused": paused}))
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
    let dir = if path.is_empty() {
        // With a root configured, that is where browsing starts.
        if browse_root.is_empty() {
            std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
        } else {
            PathBuf::from(&browse_root)
        }
    } else {
        PathBuf::from(path)
    };

    if !within_root(&dir, &browse_root) {
        return (
            403,
            json!({"error": "path is outside the configured browse root"}),
        );
    }

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
        // is_dir/is_file follow symlinks: media libraries are routinely
        // assembled out of them, so they are listed like anything else. A
        // symlink escaping the browse root is rejected on the way in.
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

/// Serialize the configuration with the auth token blanked out.
///
/// The token is what guards this endpoint, so handing it back would let anyone
/// who reached the API once walk away with the credential itself.
fn redacted(config: &AppConfig) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or_else(|_| json!({}));
    if let Some(token) = value.pointer_mut("/daemon/auth_token") {
        *token = json!("");
    }
    value
}

/// Read the full configuration, minus the auth token.
pub fn settings_get(shared: &SharedState) -> Value {
    redacted(&lock(shared).config)
}

/// Read a client-supplied configuration, keeping the live `[daemon]` block.
///
/// `browse_root` confines the file browser and `auth_token` guards every
/// endpoint here, so a client able to rewrite them could widen its own access —
/// and bind address and port need a restart regardless. Those stay editable
/// from the config file and the TUI only.
fn merged_settings(body: &Value, live: &DaemonConfig) -> Result<AppConfig, String> {
    let mut config: AppConfig =
        serde_json::from_value(body.clone()).map_err(|e| format!("invalid settings: {e}"))?;
    config.daemon = live.clone();
    config.sanitize();
    Ok(config)
}

/// Replace the configuration: sanitize, persist to config.toml, and swap the
/// live copy. Changes apply from the next analysis/encode.
pub fn settings_post(shared: &SharedState, body: &Value) -> (u16, Value) {
    let mut state = lock(shared);
    let config = match merged_settings(body, &state.config.daemon) {
        Ok(config) => config,
        Err(e) => return (400, json!({"error": e})),
    };
    if let Err(e) = config.save() {
        return (500, json!({"error": format!("failed to save: {e}")}));
    }
    let saved = redacted(&config);
    state.config = config;
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
    use super::within_root;
    use std::path::Path;

    /// An empty root is unrestricted; a configured one is escape-proof.
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

    mod settings {
        use super::super::*;

        fn guarded() -> DaemonConfig {
            DaemonConfig {
                auth_token: "s3cret".to_string(),
                browse_root: "/media".to_string(),
                ..DaemonConfig::default()
            }
        }

        /// The token guards this endpoint, so it never travels back out of it.
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

        /// A client cannot unlock the filesystem it is confined to, nor clear
        /// the credential standing between it and the API.
        #[test]
        fn the_daemon_block_survives_a_hostile_post() {
            let mut hostile = serde_json::to_value(AppConfig::default()).unwrap();
            hostile["daemon"]["browse_root"] = json!("");
            hostile["daemon"]["auth_token"] = json!("");
            hostile["daemon"]["bind_address"] = json!("0.0.0.0");

            let merged = merged_settings(&hostile, &guarded()).unwrap();
            assert_eq!(merged.daemon, guarded());
        }

        /// Ordinary settings still apply, and are still sanitized on the way in.
        #[test]
        fn non_daemon_settings_still_apply() {
            let mut body = serde_json::to_value(AppConfig::default()).unwrap();
            body["audio"]["opus_bitrate_per_channel"] = json!(9000);
            body["output"]["suffix"] = json!("../escape");

            let merged = merged_settings(&body, &DaemonConfig::default()).unwrap();
            assert_eq!(
                merged.audio.opus_bitrate_per_channel,
                crate::config::AudioConfig::MAX_PER_CHANNEL
            );
            assert_eq!(merged.output.suffix, "..escape");
        }
    }

    mod tracks {
        use super::super::*;
        use crate::daemon::state::DaemonState;
        use crate::queue::EncodingJob;
        use crate::tracks::AudioTrack;
        use std::sync::{Arc, Mutex};

        fn shared_with_job() -> (SharedState, u64) {
            let mut state = DaemonState::new(AppConfig::default());
            let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
            job.audio_tracks = (0..3)
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
                .collect();
            job.status = JobStatus::Ready;
            let id = state.queue.push(job);
            (Arc::new(Mutex::new(state)), id)
        }

        /// A client cannot mark a track for Opus without also selecting it:
        /// the resulting plan drives per-stream codec options by position, so
        /// an unselected index would land the option on the wrong stream.
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

        /// A job that is already encoding refuses edits: its tracks are
        /// baked into the running FFmpeg command.
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
    }
}
