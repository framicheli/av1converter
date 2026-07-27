use super::state::{Command, SharedState, is_terminal};
use crate::config::AppConfig;
use crate::queue::{JobStatus, collect_video_files, is_video_file};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Dashboard poll: overall daemon and encode-session status.
pub fn status(shared: &SharedState) -> Value {
    let state = shared.lock().unwrap();
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
    let state = shared.lock().unwrap();
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

/// Expand an add request into concrete video files and queue them.
pub fn queue_add(cmd_tx: &Sender<Command>, body: &Value) -> (u16, Value) {
    let Some(path) = body.get("path").and_then(Value::as_str) else {
        return (400, json!({"error": "missing 'path'"}));
    };
    let mode = body.get("mode").and_then(Value::as_str).unwrap_or("file");
    let path = PathBuf::from(path);
    if !path.exists() {
        return (400, json!({"error": "path does not exist"}));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    match mode {
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
            collect_video_files(&path, &mut files);
        }
        other => return (400, json!({"error": format!("unknown mode '{other}'")})),
    }

    if files.is_empty() {
        return (400, json!({"error": "no video files found"}));
    }
    files.sort();
    let added = files.len();
    if cmd_tx.send(Command::AddPaths(files)).is_err() {
        return (500, json!({"error": "daemon is shutting down"}));
    }
    (200, json!({"added": added}))
}

/// Remove a job unless it belongs to the running encode session.
pub fn queue_remove(shared: &SharedState, cmd_tx: &Sender<Command>, body: &Value) -> (u16, Value) {
    let Some(id) = body.get("id").and_then(Value::as_u64) else {
        return (400, json!({"error": "missing 'id'"}));
    };
    {
        let state = shared.lock().unwrap();
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
        let state = shared.lock().unwrap();
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
pub fn fs_browse(path: &str, show_hidden: bool) -> (u16, Value) {
    let dir = if path.is_empty() {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
    } else {
        PathBuf::from(path)
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
        if entry_path.is_dir() {
            if !entry_path.is_symlink() {
                dirs.push(json!({"name": name}));
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
            "parent": dir.parent().map(Path::to_string_lossy),
            "dirs": dirs,
            "files": files,
        }),
    )
}

/// Read the full configuration.
pub fn settings_get(shared: &SharedState) -> Value {
    let state = shared.lock().unwrap();
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
