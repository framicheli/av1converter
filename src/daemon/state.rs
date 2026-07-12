use crate::config::AppConfig;
use crate::queue::{EncodingJob, JobStatus, QueueState};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Queue wrapper that gives every job a stable id, so HTTP clients can
/// reference jobs while the underlying `Vec` shifts on removal.
pub struct DaemonQueue {
    /// `ids[i]` corresponds to `state.jobs[i]`; kept aligned at all times.
    ids: Vec<u64>,
    pub state: QueueState,
    next_id: u64,
}

impl DaemonQueue {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            state: QueueState::new(),
            next_id: 1,
        }
    }

    /// Add a job and return its assigned id.
    pub fn push(&mut self, job: EncodingJob) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.ids.push(id);
        self.state.jobs.push(job);
        id
    }

    pub fn ids(&self) -> &[u64] {
        &self.ids
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.ids.iter().position(|&i| i == id)
    }

    pub fn job_by_id(&self, id: u64) -> Option<&EncodingJob> {
        self.index_of(id).and_then(|i| self.state.jobs.get(i))
    }

    pub fn job_by_id_mut(&mut self, id: u64) -> Option<&mut EncodingJob> {
        self.index_of(id).and_then(|i| self.state.jobs.get_mut(i))
    }

    pub fn jobs_with_ids(&self) -> impl Iterator<Item = (u64, &EncodingJob)> {
        self.ids.iter().copied().zip(self.state.jobs.iter())
    }

    /// Remove a job by id. Returns `false` if the id is unknown.
    pub fn remove(&mut self, id: u64) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.ids.remove(index);
        self.state.jobs.remove(index);
        // Keep the "currently encoding" pointer aimed at the same job
        if index < self.state.current_job_index && self.state.current_job_index > 0 {
            self.state.current_job_index -= 1;
        }
        true
    }
}

impl Default for DaemonQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a job status is terminal (will never change again).
pub fn is_terminal(status: &JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Done
            | JobStatus::DoneWithVmaf { .. }
            | JobStatus::DoneVmafFailed { .. }
            | JobStatus::Skipped { .. }
            | JobStatus::Error { .. }
            | JobStatus::QualityWarning { .. }
    )
}

/// One `run_worker` invocation. Worker messages carry an index into the
/// session's job batch; `job_ids` maps them back to stable queue ids.
pub struct EncodeSession {
    pub job_ids: Vec<u64>,
    pub cancel_flag: Arc<AtomicBool>,
}

/// Shared daemon state: read by HTTP handlers, mutated only by the
/// orchestrator loop (except `config`, which handlers replace via command).
pub struct DaemonState {
    pub queue: DaemonQueue,
    pub config: AppConfig,
    pub encoding_active: bool,
    pub paused: bool,
    pub session: Option<EncodeSession>,
    pub started_at: Instant,
}

impl DaemonState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            queue: DaemonQueue::new(),
            config,
            encoding_active: false,
            paused: false,
            session: None,
            started_at: Instant::now(),
        }
    }

    /// Whether the job with this id belongs to the currently running session.
    pub fn in_active_session(&self, id: u64) -> bool {
        self.encoding_active
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.job_ids.contains(&id))
    }
}

/// Mutations requested by HTTP handlers, executed by the orchestrator.
pub enum Command {
    AddPaths(Vec<PathBuf>),
    RemoveJob(u64),
    SetPaused(bool),
    CancelEncoding,
    /// Already sanitized and saved by the handler; swaps the live copy.
    UpdateConfig(Box<AppConfig>),
    ClearFinished,
}

pub type SharedState = Arc<Mutex<DaemonState>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_with(n: usize) -> DaemonQueue {
        let mut q = DaemonQueue::new();
        for i in 0..n {
            q.push(EncodingJob::new(PathBuf::from(format!("/tmp/f{i}.mkv"))));
        }
        q
    }

    #[test]
    fn ids_stay_stable_across_removal() {
        let mut q = queue_with(3);
        let ids: Vec<u64> = q.ids().to_vec();
        assert_eq!(ids, vec![1, 2, 3]);

        assert!(q.remove(2));
        assert_eq!(q.ids(), &[1, 3]);
        assert_eq!(q.job_by_id(3).unwrap().path, PathBuf::from("/tmp/f2.mkv"));
        assert!(q.job_by_id(2).is_none());
        assert!(!q.remove(2));

        // New pushes never reuse ids
        let new_id = q.push(EncodingJob::new(PathBuf::from("/tmp/f9.mkv")));
        assert_eq!(new_id, 4);
    }

    #[test]
    fn removal_adjusts_current_job_index() {
        let mut q = queue_with(3);
        q.state.current_job_index = 2;
        q.remove(1);
        assert_eq!(q.state.current_job_index, 1);
        // Removing at/after the pointer leaves it alone
        q.remove(3);
        assert_eq!(q.state.current_job_index, 1);
    }

    #[test]
    fn active_session_blocks_removal_check() {
        let mut state = DaemonState::new(AppConfig::default());
        let id = state
            .queue
            .push(EncodingJob::new(PathBuf::from("/tmp/a.mkv")));
        state.session = Some(EncodeSession {
            job_ids: vec![id],
            cancel_flag: Arc::new(AtomicBool::new(false)),
        });
        assert!(!state.in_active_session(id)); // not encoding yet
        state.encoding_active = true;
        assert!(state.in_active_session(id));
        assert!(!state.in_active_session(999));
    }
}
