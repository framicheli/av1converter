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
        }
    }

    pub fn elapsed_time(&self) -> Option<Duration> {
        self.start_time.map(|start| {
            self.end_time
                .map(|end| end.duration_since(start))
                .unwrap_or_else(|| start.elapsed())
        })
    }

    pub fn overall_progress(&self) -> f32 {
        if self.total_jobs_to_encode == 0 {
            return 0.0;
        }

        let completed = self
            .jobs
            .iter()
            .filter(|j| {
                matches!(
                    j.status,
                    JobStatus::Done
                        | JobStatus::DoneWithVmaf { .. }
                        | JobStatus::QualityWarning { .. }
                )
            })
            .count();

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

        let total_progress =
            (completed as f32 * 100.0 + current_progress) / self.total_jobs_to_encode as f32;
        total_progress.min(100.0)
    }

    pub fn estimated_time_remaining(&self) -> Option<Duration> {
        let progress = self.overall_progress();
        if progress <= 0.0 || progress >= 100.0 {
            return None;
        }
        let elapsed = self.elapsed_time()?;
        let elapsed_secs = elapsed.as_secs_f64();
        let total_estimated_secs = elapsed_secs / (progress as f64 / 100.0);
        let remaining_secs = total_estimated_secs - elapsed_secs;
        if remaining_secs > 0.0 {
            Some(Duration::from_secs_f64(remaining_secs))
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
                    | JobStatus::Skipped { .. }
                    | JobStatus::Error { .. }
                    | JobStatus::QualityWarning { .. }
            )
        })
    }

    /// Get total space saved across all completed jobs
    pub fn total_space_saved(&self) -> (u64, String) {
        let total_saved: u64 = self
            .jobs
            .iter()
            .filter_map(|j| j.size_reduction().map(|(saved, _)| saved))
            .sum();
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
    }
}

impl Default for QueueState {
    fn default() -> Self {
        Self::new()
    }
}
