use super::state::{Command, SharedState, is_terminal, lock};
use crate::config::AppConfig;
use crate::queue::{JobStatus, collect_video_files, is_own_output, is_video_file};
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
pub fn queue_add(shared: &SharedState, cmd_tx: &Sender<Command>, body: &Value) -> (u16, Value) {
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

    let output_config = lock(shared).config.output.clone();

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
            files.retain(|p| !is_own_output(p, &output_config));
        }
        "folder_recursive" => {
            if !path.is_dir() {
                return (400, json!({"error": "not a directory"}));
            }
            collect_video_files(&path, &mut files);
            files.retain(|p| !is_own_output(p, &output_config));
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

    // Report what will actually be queued rather than what was found: the
    // orchestrator drops paths that are already waiting or encoding.
    let already_queued = {
        let state = lock(shared);
        let pending: Vec<PathBuf> = state
            .queue
            .jobs_with_ids()
            .filter(|(_, job)| !is_terminal(&job.status))
            .map(|(_, job)| job.path.canonicalize().unwrap_or_else(|_| job.path.clone()))
            .collect();
        files
            .iter()
            .filter(|p| {
                let canonical = p.canonicalize().unwrap_or_else(|_| (*p).clone());
                pending.contains(&canonical)
            })
            .count()
    };
    let added = files.len() - already_queued;

    if cmd_tx.send(Command::AddPaths(files)).is_err() {
        return (500, json!({"error": "daemon is shutting down"}));
    }
    (
        200,
        json!({"added": added, "already_queued": already_queued}),
    )
}

/// Remove a job unless it belongs to the running encode session.
pub fn queue_remove(shared: &SharedState, cmd_tx: &Sender<Command>, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };
    {
        let state = lock(shared);
        if state.queue.job_by_id(id).is_none() {
            return (404, json!({"error": "unknown job id"}));
        }
        if state.in_active_session(id) {
            return (
                409,
                json!({"error": "job is part of the active encode session"}),
            );
        }
    }
    let _ = cmd_tx.send(Command::RemoveJob(id));
    (200, json!({"ok": true}))
}

/// Pause/resume auto-starting new sessions (does not stop the current one).
pub fn queue_pause(cmd_tx: &Sender<Command>, body: &Value) -> (u16, Value) {
    let Some(paused) = body.get("paused").and_then(Value::as_bool) else {
        return (400, json!({"error": "missing 'paused'"}));
    };
    let _ = cmd_tx.send(Command::SetPaused(paused));
    (200, json!({"paused": paused}))
}

/// Cancel the running encode session.
pub fn queue_cancel(cmd_tx: &Sender<Command>) -> (u16, Value) {
    let _ = cmd_tx.send(Command::CancelEncoding);
    (200, json!({"ok": true}))
}

/// Drop all jobs in terminal states.
pub fn queue_clear_finished(shared: &SharedState, cmd_tx: &Sender<Command>) -> (u16, Value) {
    let removed = {
        let state = lock(shared);
        state
            .queue
            .jobs_with_ids()
            .filter(|(_, job)| is_terminal(&job.status))
            .count()
    };
    let _ = cmd_tx.send(Command::ClearFinished);
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
        } else if entry_path.is_file() {
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

/// Read the full configuration.
pub fn settings_get(shared: &SharedState) -> Value {
    let state = lock(shared);
    serde_json::to_value(&state.config).unwrap_or_else(|_| json!({}))
}

/// Replace the configuration: sanitize, persist to config.toml, and swap the
/// live copy. Non-daemon fields apply from the next analysis/encode; daemon
/// bind/port changes need a restart.
pub fn settings_post(cmd_tx: &Sender<Command>, body: &Value) -> (u16, Value) {
    let mut config: AppConfig = match serde_json::from_value(body.clone()) {
        Ok(c) => c,
        Err(e) => return (400, json!({"error": format!("invalid settings: {e}")})),
    };
    config.sanitize();
    if let Err(e) = config.save() {
        return (500, json!({"error": format!("failed to save: {e}")}));
    }
    let saved = serde_json::to_value(&config).unwrap_or_else(|_| json!({}));
    let _ = cmd_tx.send(Command::UpdateConfig(Box::new(config)));
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

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A path that cannot be resolved is refused rather than assumed safe.
    #[test]
    fn unresolvable_paths_are_refused_under_a_root() {
        assert!(!within_root(Path::new("/nonexistent/x.mkv"), "/tmp"));
    }
}
