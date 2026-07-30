use super::job::{EncodingJob, JobStatus};
use crate::utils::format_file_size;
use std::time::{Duration, Instant};

/// Overall queue state
pub struct QueueState {
    pub jobs: Vec<EncodingJob>,
    pub current_job_index: usize,
    pub config_job_index: usize,
    pub start_time: Option<Instant>,
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
