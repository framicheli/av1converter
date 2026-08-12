use super::job::{EncodingJob, JobStatus};
use crate::utils::format_file_size;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};
use tracing::warn;

/// Overall queue state
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct QueueState {
    pub jobs: Vec<EncodingJob>,
    pub current_job_index: usize,
    pub config_job_index: usize,
    /// Session timing. An `Instant` is only meaningful inside the process that
    /// took it, so it is never persisted — a reloaded queue starts a fresh
    /// clock rather than reporting an elapsed time measured against a boot
    /// that already happened.
    #[serde(skip)]
    pub start_time: Option<Instant>,
    #[serde(skip)]
    pub end_time: Option<Instant>,
    pub total_jobs_to_encode: usize,
    pub converted_count: usize,
    pub skipped_count: usize,
    pub error_count: usize,
    pub encoding_progress_done: usize,
    /// Bytes saved by finished jobs that are no longer in `jobs`. The daemon
    /// lets the queue be cleared, and the running total must survive that.
    pub cleared_saved_bytes: u64,
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

    /// Progress across the current encode session, in percent.
    ///
    /// Counted from [`Self::encoding_progress_done`] rather than by scanning the
    /// job list: the daemon lets jobs be removed from the queue mid-session, and
    /// a finished job that is no longer listed must not un-count itself.
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

        // u32::try_from avoids usize→f64 precision lint; f64::from(u32) is lossless
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
            // `try_from_secs_f64` rather than the panicking form: an estimate
            // built by dividing by a progress value approaching zero can
            // overflow `Duration`, and this runs on every dashboard poll — the
            // last place that should be able to take a request handler down.
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

    /// Get total space saved across all completed jobs, including jobs that
    /// have since been removed from the queue.
    pub fn total_space_saved(&self) -> (u64, String) {
        let listed: u64 = self
            .jobs
            .iter()
            .filter_map(|j| j.size_reduction().map(|(saved, _)| saved))
            .sum();
        let total_saved = listed.saturating_add(self.cleared_saved_bytes);
        (total_saved, format_file_size(total_saved))
    }

    /// Reset the queue for a new session
    pub fn reset(&mut self) {
        self.jobs.clear();
        self.current_job_index = 0;
        self.config_job_index = 0;
        self.start_time = None;
        self.end_time = None;
        self.total_jobs_to_encode = 0;
        self.converted_count = 0;
        self.skipped_count = 0;
        self.error_count = 0;
        self.encoding_progress_done = 0;
        self.cleared_saved_bytes = 0;
    }
}

impl Default for QueueState {
    fn default() -> Self {
        Self::new()
    }
}

/// The queue as it is written to disk.
///
/// The stable HTTP ids travel with the jobs: without them a restart would
/// renumber from 1, and a browser tab left open across it would act on
/// whichever job inherited the id it was holding.
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
    /// Whether the id list still lines up with the jobs it indexes. A file
    /// that fails this would answer every lookup with the wrong job, so it is
    /// discarded in [`load`] rather than trusted.
    fn is_consistent(&self) -> bool {
        let unique: std::collections::HashSet<_> = self.ids.iter().collect();
        self.ids.len() == self.state.jobs.len()
            && unique.len() == self.ids.len()
            && self.ids.iter().all(|id| *id < self.next_id)
    }
}

/// Borrowed counterpart of [`PersistedQueue`], so saving does not clone the
/// whole queue on its way to disk.
///
/// The field names have to match [`PersistedQueue`] exactly — they are the
/// wire format — which `queue_survives_a_round_trip` is there to catch.
#[derive(Serialize)]
pub struct QueueRef<'a> {
    pub state: &'a QueueState,
    pub ids: &'a [u64],
    pub next_id: u64,
}

/// Write the queue, atomically: a full temp file is flushed to disk and then
/// renamed over the target, so a crash mid-write leaves either the previous
/// queue or the new one, never a truncated one.
#[cfg(test)]
pub fn save(path: &Path, queue: &QueueRef<'_>) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(queue).map_err(std::io::Error::other)?;
    save_serialized(path, &json)
}

/// Write an already-serialized queue without keeping its caller's state lock
/// held across disk I/O.
pub(crate) fn save_serialized(path: &Path, json: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json)?;
        // Without this the rename can land before the contents do, which is
        // exactly the truncated file the temp-and-rename is here to prevent.
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Read the persisted queue.
///
/// Every failure — absent, unreadable, corrupt, or written by a version whose
/// shape no longer parses — yields an empty queue. A daemon that refuses to
/// start because of its own bookkeeping file would be worse than one that
/// starts with nothing queued, and "fails to parse" is the whole of the
/// version story here.
pub fn load(path: &Path) -> PersistedQueue {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PersistedQueue::default(),
        Err(e) => {
            warn!("Could not read the saved queue at {}: {e}", path.display());
            return PersistedQueue::default();
        }
    };

    match serde_json::from_slice::<PersistedQueue>(&bytes) {
        Ok(queue) if queue.is_consistent() => queue,
        Ok(_) => {
            warn!(
                "Saved queue at {} is internally inconsistent; starting empty",
                path.display()
            );
            PersistedQueue::default()
        }
        Err(e) => {
            warn!(
                "Could not parse the saved queue at {}: {e}; starting empty",
                path.display()
            );
            PersistedQueue::default()
        }
    }
}

/// Reconcile a queue reloaded from disk with a process that has just started.
///
/// Every in-flight state was being advanced by a thread that died with the old
/// process, so each has to be put somewhere the new one will pick it up again.
/// What decides that is whether the job was ever analyzed, not the status it
/// was frozen in: `Pending` means "never probed" when a job is first added, but
/// a session start stamps the same status on every job it claims, and the two
/// are indistinguishable on disk.
///
/// - `Encoding` — interrupted. `FFmpeg` writes to a `.part` sibling and only
///   renames onto the real output once it finishes, so the destination was
///   never touched and what is left behind is the scratch file. That is
///   deleted and the job is queued to encode again from the start.
/// - `Verifying` — the encode itself finished and the output is complete; only
///   the VMAF check was cut short. Re-encoding a finished file would be
///   destructive, so it is recorded as encoded-but-unverified instead.
/// - `Analyzing`, and anything else still unfinished, settles by metadata:
///   analyzed jobs go to `Ready` for the encode session to claim, and the rest
///   to `Pending` for the caller to hand back to the prober — nothing else
///   re-drives those, so a job left in either in-flight state would sit in the
///   queue forever.
///
/// `AwaitingConfig` is left alone: it is waiting for a person, not a thread.
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

        // Queues written before source identities were persisted cannot safely
        // reuse old metadata. Re-probing also turns a now-missing source into a
        // visible error instead of leaving an unstartable `Ready` job forever.
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
            JobStatus::Verifying => {
                job.status = JobStatus::DoneVmafFailed {
                    reason: "interrupted by a daemon restart".to_string(),
                };
            }
            JobStatus::Encoding { .. }
            | JobStatus::Analyzing
            | JobStatus::Pending
            | JobStatus::Ready => {
                job.status = if job.metadata.is_some() {
                    JobStatus::Ready
                } else {
                    JobStatus::Pending
                };
            }
            // Terminal states, and `AwaitingConfig`, which is waiting for a
            // person rather than for a thread that died.
            _ => {}
        }
    }

    // The session counters describe a run that is over; the reloaded jobs are
    // a fresh one. Savings already banked from cleared jobs still stand.
    queue.state.current_job_index = 0;
    queue.state.total_jobs_to_encode = 0;
    queue.state.encoding_progress_done = 0;
    queue.state.start_time = None;
    queue.state.end_time = None;
}

/// Jobs that still need analysis before they can be encoded, as
/// `(index, path)`. [`resume`] leaves exactly these in `Pending`; the daemon
/// hands them back to the prober.
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
    use std::path::PathBuf;

    fn finished_job(source: u64, output: u64) -> EncodingJob {
        let mut job = EncodingJob::new(PathBuf::from("/tmp/x.mkv"));
        job.status = JobStatus::Done;
        job.source_size = Some(source);
        job.output_size = Some(output);
        job
    }

    /// Progress follows the session counter, so a finished job leaving the
    /// queue cannot drag the bar backwards.
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

    /// Cancelled jobs count as done for the session, so a cancelled batch still
    /// reads as complete rather than stalling part-way.
    #[test]
    fn a_fully_cancelled_session_reads_as_complete() {
        let mut state = QueueState::new();
        state.total_jobs_to_encode = 3;
        state.encoding_progress_done = 3;
        assert!((state.overall_progress() - 100.0).abs() < f64::EPSILON);
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

    /// Any job past `Pending` has been probed, so a fixture standing in for one
    /// needs metadata or it does not represent a reachable state.
    fn meta() -> crate::analyzer::VideoMetadata {
        crate::analyzer::VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: crate::analyzer::HdrType::Sdr,
            dv_profile: None,
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

    /// Everything a job carries has to survive the trip, not just its path:
    /// the analysis, the track choices and the per-job options are what the
    /// user would otherwise have to make again after a restart.
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
        state.jobs.push(done);
        state.cleared_saved_bytes = 777;

        save(
            &path,
            &QueueRef {
                state: &state,
                ids: &[7, 9],
                next_id: 10,
            },
        )
        .unwrap();
        let mut back = load(&path);

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

        // Finished work reloads as it stood, so the savings total is not reset
        // by a restart.
        assert!(matches!(
            back.state.jobs[1].status,
            JobStatus::DoneWithVmaf { score } if (score - 96.5).abs() < f64::EPSILON
        ));
        assert_eq!(back.state.total_space_saved().0, 777 + 600);

        // And resuming leaves settled jobs alone.
        resume(&mut back);
        assert!(matches!(back.state.jobs[0].status, JobStatus::Ready));
        assert!(matches!(
            back.state.jobs[1].status,
            JobStatus::DoneWithVmaf { .. }
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A job persisted mid-encode restarts from the beginning, and the scratch
    /// file the dead `FFmpeg` left behind goes with it. The destination itself is
    /// never touched: an encode only renames onto it once it has finished, so
    /// anything sitting there belongs to someone else.
    #[test]
    fn an_interrupted_encode_is_requeued_and_its_partial_deleted() {
        let dir = scratch("interrupted");
        let output = dir.join("movie_av1.mkv");
        let partial = dir.join("movie_av1.part.4321_0.mkv");
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

        // `Ready`, not `Pending`: an encode session only ever claims `Ready`
        // jobs, so a re-queued encode left in `Pending` would sit there for
        // good. It was analyzed before it started, so it is ready to go again.
        assert!(matches!(queue.state.jobs[0].status, JobStatus::Ready));
        assert!(needs_analysis(&queue).is_empty());
        assert!(!partial.exists(), "the partial encode should be gone");
        assert!(output.exists(), "the destination is not ours to delete");
        assert!(bystander.exists(), "only tagged scratch files are scratch");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every reloaded job has to land somewhere a running daemon will pick it
    /// up again. Nothing re-drives `Analyzing`, and only `Ready` is claimed by
    /// an encode session, so a job left in either would never move again.
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
        // A session stamps `Pending` on every job it claims, so an analyzed
        // job can be persisted as `Pending` without ever needing a probe.
        state.jobs.push(analyzed(JobStatus::Pending));
        state.jobs.push(analyzed(JobStatus::Analyzing));
        state.jobs.push(analyzed(JobStatus::Ready));
        state.jobs.push(analyzed(JobStatus::AwaitingConfig));
        state.jobs.push(analyzed(JobStatus::Verifying));
        state.jobs.push(unanalyzed(JobStatus::Pending));
        state.jobs.push(unanalyzed(JobStatus::Analyzing));
        // Legacy queues can carry metadata but no identity. Even when their
        // source has since disappeared, they must go back through analysis so
        // the failure is reported rather than sitting in `Ready` forever.
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

        // Nothing is left in a state no thread will advance.
        assert!(
            !kinds.contains(&"analyzing"),
            "a reloaded `Analyzing` job would never be probed again"
        );
        // The two that genuinely never got analyzed are handed back to the
        // prober; the ones a session had merely claimed are not re-probed.
        assert_eq!(
            needs_analysis(&queue),
            vec![
                (5, "/tmp/fresh.mkv".to_string()),
                (6, "/tmp/fresh.mkv".to_string()),
                (7, "/tmp/av1c-missing-legacy.mkv".to_string())
            ]
        );
    }

    /// A queue file the daemon cannot make sense of must not stop it starting.
    #[test]
    fn a_corrupt_queue_file_yields_an_empty_queue() {
        let dir = scratch("corrupt");

        let truncated = dir.join("truncated.json");
        std::fs::write(
            &truncated,
            b"{\"state\":{\"jobs\":[{\"path\":\"/tmp/a.mkv\"",
        )
        .unwrap();
        assert!(load(&truncated).state.jobs.is_empty());

        let garbage = dir.join("garbage.json");
        std::fs::write(&garbage, b"\x00\x01 not json at all").unwrap();
        assert!(load(&garbage).state.jobs.is_empty());

        // Well-formed JSON of the wrong shape is no better than garbage.
        let wrong_shape = dir.join("wrong.json");
        std::fs::write(&wrong_shape, b"[1, 2, 3]").unwrap();
        assert!(load(&wrong_shape).state.jobs.is_empty());

        // An absent file is the ordinary first-run case, not an error.
        assert!(load(&dir.join("missing.json")).state.jobs.is_empty());

        // Ids that do not line up with the jobs would misroute every lookup.
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
        assert!(load(&misaligned).state.jobs.is_empty());

        // A default queue is still usable rather than merely empty.
        assert_eq!(PersistedQueue::default().next_id, 1);

        // Duplicate ids would make two rows address the same first job.
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
        assert!(load(&duplicate_ids).state.jobs.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The write is atomic, so an interrupted save can never be observed as a
    /// half-written queue: the previous file stands until the new one is whole.
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

    /// Savings from cleared jobs stay in the running total.
    #[test]
    fn cleared_savings_stay_counted() {
        let mut state = QueueState::new();
        state.jobs.push(finished_job(1000, 400));
        assert_eq!(state.total_space_saved().0, 600);

        // Simulates what DaemonQueue::remove hands over on removal.
        state.jobs.clear();
        state.cleared_saved_bytes = 600;
        assert_eq!(state.total_space_saved().0, 600);
    }
}
