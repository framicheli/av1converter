use super::job::{EncodingJob, JobStatus};
use crate::utils::format_file_size;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::warn;

/// Overall queue state
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct QueueState {
    pub jobs: Vec<EncodingJob>,
    pub current_job_index: usize,
    pub config_job_index: usize,
    /// Session timing, never persisted: a reloaded queue starts a fresh clock.
    #[serde(skip)]
    pub start_time: Option<Instant>,
    #[serde(skip)]
    pub end_time: Option<Instant>,
    pub total_jobs_to_encode: usize,
    pub converted_count: usize,
    pub skipped_count: usize,
    /// The part of `skipped_count` skipped as "Cancelled".
    pub cancelled_count: usize,
    pub error_count: usize,
    pub encoding_progress_done: usize,
    /// Net byte change from finished jobs no longer in `jobs`.
    pub cleared_saved_bytes: i128,
}

impl QueueState {
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            current_job_index: 0,
            config_job_index: 0,
            start_time: None,
            end_time: None,
            total_jobs_to_encode: 0,
            converted_count: 0,
            skipped_count: 0,
            cancelled_count: 0,
            error_count: 0,
            encoding_progress_done: 0,
            cleared_saved_bytes: 0,
        }
    }

    pub fn elapsed_time(&self) -> Option<Duration> {
        self.start_time.map(|start| {
            self.end_time
                .map_or_else(|| start.elapsed(), |end| end.duration_since(start))
        })
    }

    /// Progress across the current encode session, in percent. Counted from
    /// [`Self::encoding_progress_done`], not from the job list.
    pub fn overall_progress(&self) -> f64 {
        if self.total_jobs_to_encode == 0 {
            return 0.0;
        }

        let completed = self.encoding_progress_done.min(self.total_jobs_to_encode);

        let current_progress = self
            .jobs
            .get(self.current_job_index)
            .and_then(|j| {
                if let JobStatus::Encoding { progress } = j.status {
                    Some(progress)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);

        // Counts go through u32; f64::from(u32) is lossless.
        let done = f64::from(u32::try_from(completed).unwrap_or(u32::MAX));
        let total = f64::from(u32::try_from(self.total_jobs_to_encode).unwrap_or(u32::MAX));
        ((done * 100.0 + current_progress) / total).min(100.0)
    }

    pub fn estimated_time_remaining(&self) -> Option<Duration> {
        let progress = self.overall_progress();
        if progress <= 0.0 || progress >= 100.0 {
            return None;
        }
        let elapsed = self.elapsed_time()?;
        let elapsed_secs = elapsed.as_secs_f64();
        let total_estimated_secs = elapsed_secs / (progress / 100.0);
        let remaining_secs = total_estimated_secs - elapsed_secs;
        if remaining_secs > 0.0 {
            // A progress value near zero overflows `Duration` and yields `None`.
            Duration::try_from_secs_f64(remaining_secs).ok()
        } else {
            None
        }
    }

    /// Check if all jobs are in a terminal state
    pub fn all_completed(&self) -> bool {
        self.jobs.iter().all(|j| {
            matches!(
                j.status,
                JobStatus::Done
                    | JobStatus::DoneWithVmaf { .. }
                    | JobStatus::DoneVmafFailed { .. }
                    | JobStatus::Skipped { .. }
                    | JobStatus::Error { .. }
                    | JobStatus::QualityWarning { .. }
            )
        })
    }

    /// Net byte change across completed jobs, including removed jobs.
    pub fn total_space_saved(&self) -> (i128, String) {
        let listed: i128 = self.jobs.iter().filter_map(EncodingJob::size_change).sum();
        let total_saved = listed.saturating_add(self.cleared_saved_bytes);
        let magnitude = u64::try_from(total_saved.unsigned_abs()).unwrap_or(u64::MAX);
        let human = format_file_size(magnitude);
        (
            total_saved,
            if total_saved < 0 {
                format!("-{human}")
            } else {
                human
            },
        )
    }

    /// Move a ready job up one queue position. Work in every other state stays pinned.
    pub fn can_move_ready_up(&self, index: usize) -> bool {
        index > 0
            && self
                .jobs
                .get(index)
                .is_some_and(|job| matches!(job.status, JobStatus::Ready))
            && self
                .jobs
                .get(index - 1)
                .is_some_and(|job| matches!(job.status, JobStatus::Ready))
    }

    pub fn move_ready_up(&mut self, index: usize) -> Option<usize> {
        let previous = index.checked_sub(1)?;
        if !self.can_move_ready_up(index) {
            return None;
        }
        self.jobs.swap(index, previous);
        Some(previous)
    }

    /// Reset the queue for a new session
    pub fn reset(&mut self) {
        self.jobs.clear();
        self.reset_session();
        self.cleared_saved_bytes = 0;
    }

    /// Count `jobs` skipped with the "Cancelled" reason.
    pub fn count_cancelled(&mut self, jobs: usize) {
        self.skipped_count += jobs;
        self.cancelled_count += jobs;
    }

    /// Reset results when fresh work follows a fully settled queue.
    pub fn reset_session_if_finished(&mut self) {
        if self.all_completed() {
            self.reset_session();
        }
    }

    fn reset_session(&mut self) {
        self.current_job_index = 0;
        self.config_job_index = 0;
        self.start_time = None;
        self.end_time = None;
        self.total_jobs_to_encode = 0;
        self.converted_count = 0;
        self.skipped_count = 0;
        self.cancelled_count = 0;
        self.error_count = 0;
        self.encoding_progress_done = 0;
    }
}

impl Default for QueueState {
    fn default() -> Self {
        Self::new()
    }
}

/// The queue as it is written to disk, carrying the stable HTTP ids across a
/// restart.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedQueue {
    pub state: QueueState,
    /// `ids[i]` corresponds to `state.jobs[i]`.
    pub ids: Vec<u64>,
    pub next_id: u64,
}

impl Default for PersistedQueue {
    fn default() -> Self {
        Self {
            state: QueueState::new(),
            ids: Vec::new(),
            next_id: 1,
        }
    }
}

impl PersistedQueue {
    /// Whether the id list lines up with the jobs it indexes: one unique id
    /// per job, all below `next_id`. [`load`] discards a file that fails this.
    fn is_consistent(&self) -> bool {
        let unique: std::collections::HashSet<_> = self.ids.iter().collect();
        self.ids.len() == self.state.jobs.len() && unique.len() == self.ids.len()
    }
}

/// Borrowed counterpart of [`PersistedQueue`], for saving without cloning.
/// The field names are the wire format and must match it exactly.
#[derive(Serialize)]
pub struct QueueRef<'a> {
    pub state: &'a QueueState,
    pub ids: &'a [u64],
    pub next_id: u64,
}

/// Write the queue atomically: a temp file is flushed, then renamed over the
/// target.
#[cfg(test)]
pub fn save(path: &Path, queue: &QueueRef<'_>) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(queue).map_err(std::io::Error::other)?;
    save_serialized(path, &json)
}

/// Write an already-serialized queue to `path`.
pub(crate) fn save_serialized(path: &Path, json: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json)?;
        // The contents reach disk before the rename.
        file.sync_all()?;
    }
    let bak = path.with_extension("json.bak");
    if read_queue(path).is_some() {
        std::fs::copy(path, &bak)?;
    }
    std::fs::rename(&tmp, path)
}

fn read_queue(path: &Path) -> Option<PersistedQueue> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn!("Could not read the saved queue at {}: {e}", path.display());
            return None;
        }
    };
    match serde_json::from_slice::<PersistedQueue>(&bytes) {
        Ok(queue) if queue.is_consistent() => Some(queue),
        Ok(_) => {
            warn!(
                "Saved queue at {} is internally inconsistent",
                path.display()
            );
            None
        }
        Err(e) => {
            warn!("Could not parse the saved queue at {}: {e}", path.display());
            None
        }
    }
}

/// Read the persisted queue. A corrupt or inconsistent file is moved to
/// `queue.json.unreadable-<secs>`, and the queue falls back to `queue.json.bak`
/// from the last successful save, then to empty. Returns the queue and the
/// path the unreadable file was moved to.
pub fn load(path: &Path) -> (PersistedQueue, Option<PathBuf>) {
    if let Some(queue) = read_queue(path) {
        return (queue, None);
    }
    let preserved = preserve_unreadable(path);
    let bak = path.with_extension("json.bak");
    if bak != *path
        && let Some(queue) = read_queue(&bak)
    {
        warn!("Recovered the queue from {}", bak.display());
        return (queue, preserved);
    }
    (PersistedQueue::default(), preserved)
}

/// Copy an existing, unreadable queue file to a new
/// `<name>.unreadable-<secs>[-<n>]` next to it, then remove the original.
/// Returns the copy's path, or `None` when there is no file or the copy fails.
fn preserve_unreadable(path: &Path) -> Option<PathBuf> {
    use std::io::Write;

    let bytes = std::fs::read(path).ok()?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = path.file_name()?.to_string_lossy().into_owned();
    for n in 0u32.. {
        let suffix = if n == 0 {
            format!("{name}.unreadable-{secs}")
        } else {
            format!("{name}.unreadable-{secs}-{n}")
        };
        let target = path.with_file_name(suffix);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                warn!(
                    "Could not keep the unreadable queue at {}: {e}",
                    target.display()
                );
                return None;
            }
        };
        if let Err(e) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            warn!(
                "Could not keep the unreadable queue at {}: {e}",
                target.display()
            );
            return None;
        }
        if let Err(e) = std::fs::remove_file(path) {
            warn!(
                "Could not remove the unreadable queue {}: {e}",
                path.display()
            );
        }
        warn!("Moved the unreadable queue to {}", target.display());
        return Some(target);
    }
    None
}

/// Reconcile a queue reloaded from disk with a process that has just started.
///
/// In-flight states settle by whether the job carries metadata, not by the
/// status it was frozen in: a session start stamps `Pending` on every job it
/// claims.
///
/// - `Encoding` — the `.part` scratch file is deleted and the job re-queued to
///   encode from the start. The destination is untouched: the scratch file is
///   published to it only once the encode finishes.
/// - `Verifying` with its output on disk — recorded as encoded-but-unverified.
///   Without its output, it is handled like `Encoding`.
/// - `Analyzing`, and anything else unfinished — `Ready` when analyzed,
///   `Pending` otherwise, for the caller to hand back to the prober.
/// - `AwaitingConfig` and terminal states are left alone.
pub fn resume(queue: &mut PersistedQueue) {
    for job in &mut queue.state.jobs {
        if let JobStatus::Encoding { .. } = job.status
            && let Some(output) = job.output_path.as_deref()
        {
            for partial in crate::encoder::orphaned_partials(output) {
                match std::fs::remove_file(&partial) {
                    Ok(()) => warn!("Removed partial output {}", partial.display()),
                    Err(e) => {
                        warn!("Could not remove partial output {}: {e}", partial.display());
                    }
                }
            }
        }

        // A job with no recorded source identity is re-probed.
        if job.source_identity.is_none() && job.metadata.is_some() && !job.status.is_terminal() {
            job.metadata = None;
            job.audio_tracks.clear();
            job.subtitle_tracks.clear();
            job.track_selection = crate::tracks::TrackSelection::default();
            job.output_path = None;
            job.remux_only = false;
            job.dv_mode = None;
            job.status = JobStatus::Pending;
        }

        match job.status {
            // The file the rip was writing is incomplete; the job records the
            // failure and stops claiming its staging directory.
            JobStatus::Ripping { .. } => {
                job.temporary = false;
                job.status = JobStatus::Error {
                    message: "the rip was interrupted by a restart".to_string(),
                };
            }
            JobStatus::Verifying if job.output_path.as_deref().is_some_and(Path::exists) => {
                job.status = JobStatus::DoneVmafFailed {
                    reason: "interrupted by a daemon restart".to_string(),
                };
            }
            JobStatus::Encoding { .. }
            | JobStatus::Verifying
            | JobStatus::Analyzing
            | JobStatus::Pending
            | JobStatus::Ready => {
                job.status = if job.metadata.is_some() {
                    JobStatus::Ready
                } else {
                    JobStatus::Pending
                };
            }
            // Terminal states, and `AwaitingConfig`.
            _ => {}
        }
    }

    // Session counters start over; banked savings from cleared jobs stand.
    queue.state.reset_session();
}

/// Jobs that still need analysis, as `(index, path)`. [`resume`] leaves
/// exactly these in `Pending`; the daemon hands them back to the prober.
pub fn needs_analysis(queue: &PersistedQueue) -> Vec<(usize, String)> {
    queue
        .state
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, job)| matches!(job.status, JobStatus::Pending) && job.metadata.is_none())
        .filter_map(|(i, job)| job.path.to_str().map(|path| (i, path.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved queue whose ids run past its `next_id` is kept, with `next_id`
    /// floored past the highest id in hand.
    #[test]
    fn a_queue_whose_ids_run_past_next_id_keeps_its_jobs() {
        let dir = std::env::temp_dir().join(format!("av1c_ids_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        let job = crate::queue::EncodingJob::new(std::path::PathBuf::from("/movies/a.mkv"));
        let state = QueueState {
            jobs: vec![job],
            ..QueueState::default()
        };
        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[7],
                next_id: 2,
            },
        )
        .unwrap();

        let (loaded, unreadable) = load(&path);

        assert!(unreadable.is_none());
        assert_eq!(loaded.ids, vec![7]);
        assert_eq!(loaded.state.jobs.len(), 1);
        let mut queue = crate::daemon::state::DaemonQueue::from_persisted(loaded);
        let next = queue.push(crate::queue::EncodingJob::new(std::path::PathBuf::from(
            "/movies/b.mkv",
        )));
        assert_eq!(next, 8);
        let _ = std::fs::remove_dir_all(&dir);
    }
    use std::path::PathBuf;

    fn finished_job(source: u64, output: u64) -> EncodingJob {
        let mut job = EncodingJob::new(PathBuf::from("/tmp/x.mkv"));
        job.status = JobStatus::Done;
        job.source_size = Some(source);
        job.output_size = Some(output);
        job
    }

    #[test]
    fn only_adjacent_ready_jobs_can_move_up() {
        let mut state = QueueState::new();
        let mut first = EncodingJob::new(PathBuf::from("first.mkv"));
        first.status = JobStatus::Ready;
        let mut second = EncodingJob::new(PathBuf::from("second.mkv"));
        second.status = JobStatus::Ready;
        state.jobs = vec![first, second];

        assert_eq!(state.move_ready_up(1), Some(0));
        assert_eq!(state.jobs[0].filename(), "second.mkv");
        state.jobs[0].status = JobStatus::Encoding { progress: 0.0 };
        assert_eq!(state.move_ready_up(1), None);
    }

    /// Progress holds when a finished job leaves the queue.
    #[test]
    fn progress_survives_jobs_leaving_the_queue() {
        let mut state = QueueState::new();
        state.total_jobs_to_encode = 2;
        state.encoding_progress_done = 2;
        state.jobs.push(finished_job(100, 40));

        assert!((state.overall_progress() - 100.0).abs() < f64::EPSILON);
        state.jobs.clear();
        assert!((state.overall_progress() - 100.0).abs() < f64::EPSILON);
    }

    /// Cancelled jobs count as done, and a cancelled batch reads as complete.
    #[test]
    fn a_fully_cancelled_session_reads_as_complete() {
        let mut state = QueueState::new();
        state.total_jobs_to_encode = 3;
        for _ in 0..3 {
            let mut job = EncodingJob::new(PathBuf::from("cancelled.mkv"));
            job.status = JobStatus::Skipped {
                reason: "Cancelled".to_string(),
            };
            state.jobs.push(job);
        }
        state.count_cancelled(3);
        state.encoding_progress_done = state.cancelled_count;

        assert_eq!(state.cancelled_count, 3);
        assert_eq!(state.skipped_count, 3);
        assert!(state.all_completed());
        assert!((state.overall_progress() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn total_space_accounts_for_outputs_that_grew() {
        let mut state = QueueState::new();
        state.jobs.push(finished_job(100, 40));
        state.jobs.push(finished_job(100, 180));

        assert_eq!(state.total_space_saved(), (-20, "-20 B".to_string()));
    }

    /// A scratch directory of this test's own, cleaned up on the way out.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("av1c_persist_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn persisted(state: QueueState, ids: Vec<u64>, next_id: u64) -> PersistedQueue {
        PersistedQueue {
            state,
            ids,
            next_id,
        }
    }

    /// Metadata for fixtures standing in for a job past `Pending`.
    fn meta() -> crate::analyzer::VideoMetadata {
        crate::analyzer::VideoMetadata {
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
        }
    }

    fn source_identity() -> crate::queue::SourceIdentity {
        let executable = std::env::current_exe().unwrap();
        crate::queue::SourceIdentity::from_metadata(&std::fs::metadata(executable).unwrap())
    }

    /// A job's analysis, track choices and per-job options all survive the
    /// round trip, not just its path.
    #[test]
    fn queue_survives_a_round_trip() {
        let dir = scratch("round_trip");
        let path = dir.join("queue.json");

        let mut state = QueueState::new();
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
        job.status = JobStatus::Ready;
        job.metadata = Some(meta());
        job.source_identity = Some(source_identity());
        job.output_path = Some(PathBuf::from("/tmp/movie_av1.mkv"));
        job.track_selection.audio_indices = vec![0, 2];
        job.track_selection.audio_to_opus = vec![2];
        job.track_selection.subtitle_indices = vec![1];
        job.remux_only = true;
        job.dv_mode = Some(crate::analyzer::DvMode::KeepDolbyVision);
        job.source_size = Some(4096);
        state.jobs.push(job);

        let mut done = EncodingJob::new(PathBuf::from("/tmp/other.mkv"));
        done.status = JobStatus::DoneWithVmaf { score: 96.5 };
        done.source_size = Some(1000);
        done.output_size = Some(400);
        done.source_kept_reason = Some(crate::encoder::KeepReason::AudioTranscoded);
        state.jobs.push(done);
        state.cleared_saved_bytes = 777;
        state.converted_count = 7;
        state.skipped_count = 9;
        state.error_count = 3;

        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[7, 9],
                next_id: 10,
            },
        )
        .unwrap();
        let (mut back, _) = load(&path);

        assert_eq!(back.ids, vec![7, 9]);
        assert_eq!(back.next_id, 10);
        assert_eq!(back.state.cleared_saved_bytes, 777);
        assert_eq!(back.state.jobs.len(), 2);

        let first = &back.state.jobs[0];
        assert_eq!(first.path, PathBuf::from("/tmp/movie.mkv"));
        assert_eq!(first.output_path, Some(PathBuf::from("/tmp/movie_av1.mkv")));
        assert_eq!(first.track_selection.audio_indices, vec![0, 2]);
        assert_eq!(first.track_selection.audio_to_opus, vec![2]);
        assert_eq!(first.track_selection.subtitle_indices, vec![1]);
        assert!(first.remux_only);
        assert_eq!(
            first.dv_mode,
            Some(crate::analyzer::DvMode::KeepDolbyVision)
        );
        assert!(matches!(first.status, JobStatus::Ready));

        // Finished work reloads as it stood, savings total included.
        assert!(matches!(
            back.state.jobs[1].status,
            JobStatus::DoneWithVmaf { score } if (score - 96.5).abs() < f64::EPSILON
        ));
        assert_eq!(back.state.total_space_saved().0, 777 + 600);
        assert_eq!(
            back.state.jobs[1].source_kept_reason,
            Some(crate::encoder::KeepReason::AudioTranscoded)
        );

        // A queue written before the keep reason existed still loads.
        let mut older = serde_json::to_value(&back.state.jobs[1]).unwrap();
        older.as_object_mut().unwrap().remove("source_kept_reason");
        let older: EncodingJob = serde_json::from_value(older).unwrap();
        assert_eq!(older.source_kept_reason, None);

        // And resuming leaves settled jobs alone.
        resume(&mut back);
        assert!(matches!(back.state.jobs[0].status, JobStatus::Ready));
        assert!(matches!(
            back.state.jobs[1].status,
            JobStatus::DoneWithVmaf { .. }
        ));
        assert_eq!(back.state.converted_count, 0);
        assert_eq!(back.state.skipped_count, 0);
        assert_eq!(back.state.error_count, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A job persisted mid-encode restarts from the beginning and its scratch
    /// file is deleted. The destination and untagged files are left alone.
    #[test]
    fn an_interrupted_encode_is_requeued_and_its_partial_deleted() {
        let dir = scratch("interrupted");
        let output = dir.join("movie_av1.mkv");
        // A pid no live process can hold.
        let partial = dir.join("movie_av1.part.4294967294_0.mkv");
        let bystander = dir.join("movie_av1.part.mine.mkv");
        std::fs::write(&output, b"a finished file from an earlier run").unwrap();
        std::fs::write(&partial, b"half an encode").unwrap();
        std::fs::write(&bystander, b"not scratch").unwrap();

        let mut job = EncodingJob::new(dir.join("movie.mkv"));
        job.status = JobStatus::Encoding { progress: 42.0 };
        job.metadata = Some(meta());
        job.source_identity = Some(source_identity());
        job.output_path = Some(output.clone());
        let mut state = QueueState::new();
        state.jobs.push(job);
        let mut queue = persisted(state, vec![1], 2);

        resume(&mut queue);

        // `Ready`, not `Pending`: an encode session only claims `Ready` jobs.
        assert!(matches!(queue.state.jobs[0].status, JobStatus::Ready));
        assert!(needs_analysis(&queue).is_empty());
        assert!(!partial.exists(), "the partial encode should be gone");
        assert!(output.exists(), "the destination is not ours to delete");
        assert!(bystander.exists(), "only tagged scratch files are scratch");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every reloaded job lands in a state something will advance: `Ready`,
    /// `Pending`, `AwaitingConfig` or terminal — never `Analyzing`.
    #[test]
    fn no_reloaded_job_is_left_stranded() {
        let analyzed = |status| {
            let mut job = EncodingJob::new(PathBuf::from("/tmp/x.mkv"));
            job.metadata = Some(meta());
            job.source_identity = Some(source_identity());
            job.status = status;
            job
        };
        let unanalyzed = |status| {
            let mut job = EncodingJob::new(PathBuf::from("/tmp/fresh.mkv"));
            job.status = status;
            job
        };

        let mut state = QueueState::new();
        // An analyzed job can be persisted as `Pending`: a session stamps that
        // on every job it claims.
        state.jobs.push(analyzed(JobStatus::Pending));
        state.jobs.push(analyzed(JobStatus::Analyzing));
        state.jobs.push(analyzed(JobStatus::Ready));
        state.jobs.push(analyzed(JobStatus::AwaitingConfig));
        let finished_output =
            std::env::temp_dir().join(format!("av1c-stranded-out-{}.mkv", std::process::id()));
        std::fs::write(&finished_output, b"encoded").unwrap();
        let mut verifying = analyzed(JobStatus::Verifying);
        verifying.output_path = Some(finished_output.clone());
        state.jobs.push(verifying);
        state.jobs.push(unanalyzed(JobStatus::Pending));
        state.jobs.push(unanalyzed(JobStatus::Analyzing));
        // A job with metadata but no identity goes back through analysis even
        // when the source has disappeared.
        let mut legacy = EncodingJob::new(PathBuf::from("/tmp/av1c-missing-legacy.mkv"));
        legacy.metadata = Some(meta());
        legacy.status = JobStatus::Ready;
        state.jobs.push(legacy);
        let ids = (1..=8).collect::<Vec<u64>>();
        let mut queue = persisted(state, ids, 9);

        resume(&mut queue);

        let kinds: Vec<&str> = queue
            .state
            .jobs
            .iter()
            .map(|j| match j.status {
                JobStatus::Pending => "pending",
                JobStatus::Analyzing => "analyzing",
                JobStatus::AwaitingConfig => "awaiting",
                JobStatus::Ready => "ready",
                JobStatus::DoneVmafFailed { .. } => "unverified",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "ready",
                "ready",
                "ready",
                "awaiting",
                "unverified",
                "pending",
                "pending",
                "pending"
            ]
        );

        assert!(
            !kinds.contains(&"analyzing"),
            "a reloaded `Analyzing` job would never be probed again"
        );
        let _ = std::fs::remove_file(&finished_output);
        // Only the never-analyzed ones go back to the prober.
        assert_eq!(
            needs_analysis(&queue),
            vec![
                (5, "/tmp/fresh.mkv".to_string()),
                (6, "/tmp/fresh.mkv".to_string()),
                (7, "/tmp/av1c-missing-legacy.mkv".to_string())
            ]
        );
    }

    /// A job stopped during VMAF whose output is gone is encoded again, not
    /// recorded as finished.
    #[test]
    fn a_verifying_job_without_its_output_is_encoded_again() {
        let mut job = EncodingJob::new(PathBuf::from("/staging/rip-a1/DISC_t00.mkv"));
        job.metadata = Some(meta());
        job.source_identity = Some(source_identity());
        job.temporary = true;
        job.status = JobStatus::Verifying;
        job.output_path = Some(PathBuf::from("/av1c/does/not/exist/DISC_t00_av1.mkv"));
        let mut state = QueueState::new();
        state.jobs.push(job);
        let mut queue = persisted(state, vec![1], 2);

        resume(&mut queue);

        assert!(matches!(queue.state.jobs[0].status, JobStatus::Ready));
        assert!(queue.state.jobs[0].temporary);
    }

    /// A rip cut short by a restart records the failure and stops claiming its
    /// staging directory.
    #[test]
    fn an_interrupted_rip_does_not_come_back_as_a_job() {
        let mut job = EncodingJob::new(PathBuf::from("/staging/rip-a1/DISC_t00.mkv"));
        job.status = JobStatus::Ripping { progress: 0.0 };
        job.temporary = true;
        let mut state = QueueState::new();
        state.jobs.push(job);
        let mut queue = persisted(state, vec![1], 2);

        resume(&mut queue);

        assert!(matches!(
            queue.state.jobs[0].status,
            JobStatus::Error { .. }
        ));
        assert!(
            !queue.state.jobs[0].temporary,
            "the sweep must be free to delete the partial rip"
        );
        assert!(needs_analysis(&queue).is_empty());
    }

    /// Unparseable, misshapen and inconsistent queue files all load as empty.
    #[test]
    fn a_corrupt_queue_file_yields_an_empty_queue() {
        let dir = scratch("corrupt");

        let truncated = dir.join("truncated.json");
        std::fs::write(
            &truncated,
            b"{\"state\":{\"jobs\":[{\"path\":\"/tmp/a.mkv\"",
        )
        .unwrap();
        assert!(load(&truncated).0.state.jobs.is_empty());

        let garbage = dir.join("garbage.json");
        std::fs::write(&garbage, b"\x00\x01 not json at all").unwrap();
        assert!(load(&garbage).0.state.jobs.is_empty());

        // Well-formed JSON of the wrong shape.
        let wrong_shape = dir.join("wrong.json");
        std::fs::write(&wrong_shape, b"[1, 2, 3]").unwrap();
        assert!(load(&wrong_shape).0.state.jobs.is_empty());

        // An absent file: the ordinary first-run case.
        assert!(load(&dir.join("missing.json")).0.state.jobs.is_empty());

        // Ids that do not line up with the jobs.
        let misaligned = dir.join("misaligned.json");
        let mut state = QueueState::new();
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/a.mkv")));
        save(
            &misaligned,
            &QueueRef {
                state: &state,
                ids: &[1, 2],
                next_id: 3,
            },
        )
        .unwrap();
        assert!(load(&misaligned).0.state.jobs.is_empty());

        // The empty queue is usable, not just empty.
        assert_eq!(PersistedQueue::default().next_id, 1);

        // Duplicate ids.
        let duplicate_ids = dir.join("duplicate-ids.json");
        let mut state = QueueState::new();
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/a.mkv")));
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/b.mkv")));
        save(
            &duplicate_ids,
            &QueueRef {
                state: &state,
                ids: &[1, 1],
                next_id: 2,
            },
        )
        .unwrap();
        assert!(load(&duplicate_ids).0.state.jobs.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An unreadable queue file keeps its bytes in a side file through the
    /// saves that follow, and the backup keeps the last good queue.
    #[test]
    fn an_unreadable_queue_file_is_kept_through_later_saves() {
        let dir = scratch("unreadable");
        let path = dir.join("queue.json");
        let bak = dir.join("queue.json.bak");
        let mut state = QueueState::new();
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/keep.mkv")));
        let good = QueueRef {
            state: &state,
            ids: &[1],
            next_id: 2,
        };
        save(&path, &good).unwrap();
        save(&path, &good).unwrap();
        let good_bytes = std::fs::read(&bak).unwrap();

        let corrupt = b"{\"state\":{\"jobs\":[{\"path\":\"/tmp/a.mkv\"";
        std::fs::write(&path, corrupt).unwrap();

        // A save over the unreadable file leaves the backup alone.
        let empty = QueueState::new();
        let empty_ref = QueueRef {
            state: &empty,
            ids: &[],
            next_id: 1,
        };
        std::fs::write(&path, corrupt).unwrap();
        save(&path, &empty_ref).unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), good_bytes);

        std::fs::write(&path, corrupt).unwrap();
        let (queue, preserved) = load(&path);
        let preserved = preserved.expect("the unreadable file is kept");
        assert_eq!(queue.ids, vec![1]);
        assert!(
            preserved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("queue.json.unreadable-")
        );
        save(&path, &empty_ref).unwrap();
        save(&path, &empty_ref).unwrap();
        assert_eq!(std::fs::read(&preserved).unwrap(), corrupt);

        // A second unreadable file gets a name of its own.
        std::fs::write(&path, b"garbage").unwrap();
        let (_, second) = load(&path);
        let second = second.expect("the second unreadable file is kept");
        assert_ne!(second, preserved);
        assert_eq!(std::fs::read(&second).unwrap(), b"garbage");
        assert_eq!(std::fs::read(&preserved).unwrap(), corrupt);

        // A readable or absent file reports nothing.
        save(&path, &good).unwrap();
        assert_eq!(load(&path).1, None);
        assert_eq!(load(&dir.join("missing.json")).1, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Repeated saves leave no `.tmp` file behind.
    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let dir = scratch("atomic");
        let path = dir.join("queue.json");
        let state = QueueState::new();

        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[],
                next_id: 1,
            },
        )
        .unwrap();
        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[],
                next_id: 1,
            },
        )
        .unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| std::path::Path::new(name).extension() == Some("tmp".as_ref()))
            .collect();
        assert!(leftovers.is_empty(), "left temporary files: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_queue_reloads_from_the_backup() {
        let dir = scratch("bak");
        let path = dir.join("queue.json");
        let mut state = QueueState::new();
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/keep.mkv")));
        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[1],
                next_id: 2,
            },
        )
        .unwrap();
        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[1],
                next_id: 2,
            },
        )
        .unwrap();
        std::fs::write(&path, b"{not json").unwrap();

        let (back, _) = load(&path);
        assert_eq!(back.ids, vec![1]);
        assert_eq!(back.state.jobs[0].path, PathBuf::from("/tmp/keep.mkv"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Savings from cleared jobs stay in the running total.
    #[test]
    fn cleared_savings_stay_counted() {
        let mut queue = crate::daemon::state::DaemonQueue::new();
        let id = queue.push(finished_job(1000, 400));
        assert_eq!(queue.state.total_space_saved().0, 600);

        assert!(queue.remove(id));
        assert!(queue.state.jobs.is_empty());
        assert_eq!(queue.state.cleared_saved_bytes, 600);
        assert_eq!(queue.state.total_space_saved().0, 600);
        assert!(!queue.remove(id));
    }
}
