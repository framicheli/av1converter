use crate::config::AppConfig;
use crate::disc::{DiscDrive, DiscSource, DiscTitle};
use crate::queue::{EncodingJob, JobStatus, QueueState};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
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

    /// Borrow the queue in the shape it is persisted in.
    pub fn as_persistable(&self) -> crate::queue::QueueRef<'_> {
        crate::queue::QueueRef {
            state: &self.state,
            ids: &self.ids,
            next_id: self.next_id,
        }
    }

    /// Rebuild from a queue read back off disk, flooring `next_id` past every
    /// id in hand.
    pub fn from_persisted(persisted: crate::queue::PersistedQueue) -> Self {
        let next_id = persisted
            .ids
            .iter()
            .copied()
            .max()
            .map_or(persisted.next_id, |highest| {
                persisted.next_id.max(highest + 1)
            });
        Self {
            ids: persisted.ids,
            state: persisted.state,
            next_id,
        }
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        debug_assert_eq!(
            self.ids.len(),
            self.state.jobs.len(),
            "ids and jobs drifted apart; every lookup by id is now wrong"
        );
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

    pub fn move_ready_up(&mut self, id: u64) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let Some(previous) = self.state.move_ready_up(index) else {
            return false;
        };
        self.ids.swap(index, previous);
        true
    }

    /// Remove a job by id. Returns `false` if the id is unknown.
    pub fn remove(&mut self, id: u64) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.ids.remove(index);
        let job = self.state.jobs.remove(index);
        // Removed jobs keep their size change in the running total.
        if let Some(change) = job.size_change() {
            self.state.cleared_saved_bytes = self.state.cleared_saved_bytes.saturating_add(change);
        }
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
    status.is_terminal()
}

/// One `run_worker` invocation. Worker messages carry an index into the
/// session's job batch; `job_ids` maps them back to stable queue ids.
pub struct EncodeSession {
    pub job_ids: Vec<u64>,
    pub cancel_flag: Arc<AtomicBool>,
}

/// Disc scanning and ripping, as the API sees it.
///
/// One run at a time, scan or rip: there is one drive. `active` is set when a
/// run starts and cleared only by the event that ends it, so a cancelled run
/// cannot have its trailing events applied to the next one.
#[derive(Default)]
pub struct DiscSession {
    /// Drives from the last listing. The only drive ids a request may name.
    pub drives: Vec<DiscDrive>,
    /// What was last scanned, and what was found on it.
    pub scanned_source: Option<DiscSource>,
    pub disc_type: Option<String>,
    pub titles: Vec<DiscTitle>,
    /// Queue ids of the titles being extracted, in the order requested.
    pub job_ids: Vec<u64>,
    pub cancel_flag: Option<Arc<AtomicBool>>,
    pub active: bool,
    pub scanning: bool,
    /// Why the last run stopped, in the user's language.
    pub error: Option<String>,
}

impl DiscSession {
    /// Ask the running scan or rip to stop. The run stays active until its
    /// own event says otherwise.
    pub fn cancel(&self) {
        if let Some(flag) = self.cancel_flag.as_ref() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// A run has ended, however it ended.
    pub fn settle(&mut self) {
        self.active = false;
        self.scanning = false;
        self.job_ids.clear();
        self.cancel_flag = None;
    }
}

/// Shared daemon state. HTTP handlers commit short mutations under the mutex;
/// analysis and worker results are applied by the orchestrator loop.
pub struct DaemonState {
    pub queue: DaemonQueue,
    pub config: AppConfig,
    pub encoding_active: bool,
    pub recursive_scan_active: bool,
    pub session: Option<EncodeSession>,
    pub disc: DiscSession,
    pub started_at: Instant,
    pub encode_worker: Option<JoinHandle<()>>,
    pub disc_worker: Option<JoinHandle<()>>,
    /// Same flag the HTTP accept loop watches; mutations refuse once set.
    pub shutting_down: Arc<AtomicBool>,
    /// Replaced when new files are queued so a cancelled probe keeps dying.
    pub analysis_cancel: Arc<AtomicBool>,
}

impl DaemonState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            queue: DaemonQueue::new(),
            config,
            encoding_active: false,
            recursive_scan_active: false,
            session: None,
            disc: DiscSession::default(),
            started_at: Instant::now(),
            encode_worker: None,
            disc_worker: None,
            shutting_down: Arc::new(AtomicBool::new(false)),
            analysis_cancel: Arc::new(AtomicBool::new(false)),
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

pub type SharedState = Arc<Mutex<DaemonState>>;

/// Lock the shared state, recovering from a poisoned mutex.
///
/// A panic in one HTTP handler must not turn the daemon into a process that
/// answers nothing for the rest of its life, so a poisoned lock is taken up
/// again rather than propagated.
///
/// This is a judgement, not a proof of safety. Almost everything behind the
/// mutex is plain data that a half-finished mutation leaves merely stale — but
/// [`DaemonQueue`] does hold one real invariant, `ids[i]` against
/// `state.jobs[i]`, and it is maintained by two `Vec` operations in a row
/// rather than atomically. A panic between them would misalign the two for
/// good, and every later lookup by id would answer with the wrong job. In
/// practice neither operation can panic (`push` only on allocation failure,
/// `remove` only on an index `index_of` just validated), and `index_of`
/// debug-asserts the alignment; the trade is still worth naming.
pub fn lock(shared: &SharedState) -> std::sync::MutexGuard<'_, DaemonState> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

    /// Ids keep going up across a restart, never repeating one already used.
    #[test]
    fn reloading_never_hands_out_an_id_twice() {
        let mut state = crate::queue::QueueState::new();
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/a.mkv")));
        state
            .jobs
            .push(EncodingJob::new(PathBuf::from("/tmp/b.mkv")));

        let mut q = DaemonQueue::from_persisted(crate::queue::PersistedQueue {
            state,
            ids: vec![4, 8],
            // Deliberately stale, as a truncated or hand-edited file might be.
            next_id: 2,
        });

        assert_eq!(q.push(EncodingJob::new(PathBuf::from("/tmp/c.mkv"))), 9);
        assert_eq!(q.job_by_id(4).unwrap().path, PathBuf::from("/tmp/a.mkv"));
        assert_eq!(q.job_by_id(8).unwrap().path, PathBuf::from("/tmp/b.mkv"));
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
    fn moving_a_ready_job_keeps_its_id_attached() {
        let mut q = queue_with(2);
        for job in &mut q.state.jobs {
            job.status = JobStatus::Ready;
        }

        assert!(q.move_ready_up(2));
        assert_eq!(q.ids(), &[2, 1]);
        assert_eq!(q.job_by_id(2).unwrap().path, PathBuf::from("/tmp/f1.mkv"));
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
