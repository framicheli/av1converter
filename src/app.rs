use crate::analyzer::{self, AnalysisResult, is_av1_codec};
use crate::config::{AppConfig, TrackPresetConfig};
use crate::error::AppError;
use crate::queue::{
    EncodingJob, JobStatus, QueueState, WorkerJob, WorkerMessage, is_video_file, run_worker,
};
use crate::utils::DependencyStatus;
use ratatui::widgets::ListState;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Instant;
use tracing::info;

/// Application screens
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Home,
    FileExplorer { select_folder: bool },
    FileConfirm,
    TrackConfig,
    Queue,
    Finish,
    Configuration,
}

/// File selection mode
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionMode {
    File,
    Folder,
    FolderRecursive,
}

/// Track configuration focus
#[derive(Debug, Clone, PartialEq)]
pub enum TrackFocus {
    Audio,
    Subtitle,
    Confirm,
}

/// Confirmation dialog action
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfirmAction {
    CancelEncoding,
    ExitApp,
    AbandonTrackConfig,
    DiscardConfigChanges,
}

pub const HOME_MENU: &[&str] = &[
    "Open Video File",
    "Open Folder",
    "Open Folder (Recursive)",
    "Configuration",
    "Quit",
];

/// Main application state
pub struct App {
    pub current_screen: Screen,
    pub should_quit: bool,
    pub selection_mode: SelectionMode,

    // File explorer
    pub current_dir: PathBuf,
    pub dir_entries: Vec<PathBuf>,
    pub explorer_index: usize,
    pub explorer_list_state: ListState,
    // Queue state (replaces Vec<VideoFile>)
    pub queue: QueueState,
    pub queue_cursor: usize,
    pub queue_list_state: ListState,

    // Track config
    pub track_focus: TrackFocus,
    pub audio_cursor: usize,
    pub subtitle_cursor: usize,
    pub audio_list_state: ListState,
    pub subtitle_list_state: ListState,

    // Home menu
    pub home_index: usize,

    // Multi-file selection
    pub selected_files: Vec<PathBuf>,
    pub file_confirm_scroll: usize,
    pub file_confirm_list_state: ListState,

    // Encoding
    pub encoding_active: bool,
    pub progress_receiver: Option<Receiver<WorkerMessage>>,
    pub cancel_flag: Arc<AtomicBool>,

    // Background analysis
    pub analysis_receiver: Option<Receiver<Vec<Result<AnalysisResult, AppError>>>>,
    /// Ask the analysis thread to stop spawning new ffprobe calls
    pub analysis_cancel_flag: Arc<AtomicBool>,

    // Configuration
    pub config: AppConfig,
    pub deps: bool,

    // UI state
    pub message: Option<String>,
    pub message_expiry: Option<Instant>,
    pub confirm_dialog: Option<(ConfirmAction, bool)>,

    // Config screen state
    pub config_selected: usize,
    pub config_edit_buffer: Option<String>,
    pub config_snapshot: Option<AppConfig>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map_or_else(
                    || {
                        if cfg!(windows) {
                            PathBuf::from("C:\\")
                        } else {
                            PathBuf::from("/")
                        }
                    },
                    PathBuf::from,
                )
        });
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let mut audio_list_state = ListState::default();
        audio_list_state.select(Some(0));
        let mut subtitle_list_state = ListState::default();
        subtitle_list_state.select(Some(0));
        let mut queue_list_state = ListState::default();
        queue_list_state.select(Some(0));
        let mut file_confirm_list_state = ListState::default();
        file_confirm_list_state.select(Some(0));

        let config = AppConfig::load();
        let deps = DependencyStatus::check();

        info!("Using encoder: {}", config.encoder);

        Self {
            current_screen: Screen::Home,
            should_quit: false,
            selection_mode: SelectionMode::File,
            current_dir,
            dir_entries: Vec::new(),
            explorer_index: 0,
            explorer_list_state: list_state,
            queue: QueueState::new(),
            queue_cursor: 0,
            queue_list_state,
            track_focus: TrackFocus::Audio,
            audio_cursor: 0,
            subtitle_cursor: 0,
            audio_list_state,
            subtitle_list_state,
            home_index: 0,
            selected_files: Vec::new(),
            file_confirm_scroll: 0,
            file_confirm_list_state,
            encoding_active: false,
            progress_receiver: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            analysis_receiver: None,
            analysis_cancel_flag: Arc::new(AtomicBool::new(false)),
            config,
            deps,
            message: None,
            message_expiry: None,
            confirm_dialog: None,
            config_selected: 0,
            config_edit_buffer: None,
            config_snapshot: None,
        }
    }

    // Message handling

    pub fn set_message(&mut self, msg: &str) {
        self.message = Some(msg.to_string());
        self.message_expiry = None;
    }

    pub fn set_timed_message(&mut self, msg: &str, secs: u64) {
        self.message = Some(msg.to_string());
        self.message_expiry = Some(Instant::now() + std::time::Duration::from_secs(secs));
    }

    pub fn clear_message(&mut self) {
        self.message = None;
        self.message_expiry = None;
    }

    pub fn tick_message(&mut self) {
        if let Some(expiry) = self.message_expiry
            && Instant::now() >= expiry
        {
            self.message = None;
            self.message_expiry = None;
        }
    }

    // Navigation

    pub fn navigate_to_home(&mut self) {
        self.current_screen = Screen::Home;
        self.home_index = 0;
        self.selected_files.clear();
    }

    pub fn navigate_to_explorer(&mut self, select_folder: bool, recursive: bool) {
        self.selection_mode = if !select_folder {
            SelectionMode::File
        } else if recursive {
            SelectionMode::FolderRecursive
        } else {
            SelectionMode::Folder
        };
        self.refresh_dir_entries();
        self.current_screen = Screen::FileExplorer { select_folder };
    }

    pub fn navigate_to_track_config(&mut self) {
        self.reset_track_config_cursor();
        self.current_screen = Screen::TrackConfig;
    }

    /// Reset track focus/cursors to match the job now at `config_job_index`.
    fn reset_track_config_cursor(&mut self) {
        let audio_count = self
            .current_config_job()
            .map_or(0, |j| j.audio_tracks.len());
        let subtitle_count = self
            .current_config_job()
            .map_or(0, |j| j.subtitle_tracks.len());
        self.track_focus = if audio_count > 0 {
            TrackFocus::Audio
        } else if subtitle_count > 0 {
            TrackFocus::Subtitle
        } else {
            TrackFocus::Confirm
        };
        self.audio_cursor = 0;
        self.subtitle_cursor = 0;
    }

    pub fn navigate_to_queue(&mut self) {
        self.queue_cursor = 0;
        self.current_screen = Screen::Queue;
    }

    pub fn navigate_to_finish(&mut self) {
        // Update output sizes for all jobs that produced an output file
        for job in &mut self.queue.jobs {
            if matches!(
                job.status,
                JobStatus::Done
                    | JobStatus::DoneWithVmaf { .. }
                    | JobStatus::DoneVmafFailed { .. }
                    | JobStatus::QualityWarning { .. }
            ) && let Some(ref output_path) = job.output_path
            {
                job.output_size = std::fs::metadata(output_path).ok().map(|m| m.len());
            }
        }
        self.current_screen = Screen::Finish;
    }

    pub fn navigate_to_configuration(&mut self) {
        self.config_selected = 0;
        self.config_snapshot = Some(self.config.clone());
        self.current_screen = Screen::Configuration;
    }

    /// Whether the live config has diverged from the snapshot taken on entry
    /// to the Configuration screen
    pub fn config_is_dirty(&self) -> bool {
        self.config_snapshot
            .as_ref()
            .is_some_and(|s| *s != self.config)
    }

    pub fn navigate_to_file_confirm(&mut self) {
        self.file_confirm_scroll = 0;
        self.current_screen = Screen::FileConfirm;
    }

    /// Move the queue list cursor up (`forward = false`) or down (`forward =
    /// true`), clamped within the job list bounds.
    pub fn queue_move_cursor(&mut self, forward: bool) {
        if forward {
            if self.queue_cursor < self.queue.jobs.len().saturating_sub(1) {
                self.queue_cursor += 1;
            }
        } else if self.queue_cursor > 0 {
            self.queue_cursor -= 1;
        }
    }

    // File explorer

    pub fn refresh_dir_entries(&mut self) {
        self.dir_entries.clear();

        // Add parent directory
        if let Some(parent) = self.current_dir.parent()
            && parent != self.current_dir
        {
            self.dir_entries.push(PathBuf::from(".."));
        }

        // Read directory contents
        if let Ok(entries) = std::fs::read_dir(&self.current_dir) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir() || is_video_file(p))
                .collect();

            // Sort: directories first, then files
            paths.sort_by(|a, b| match (a.is_dir(), b.is_dir()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            });

            self.dir_entries.extend(paths);
        }

        self.explorer_index = 0;
        self.explorer_list_state.select(Some(0));
    }

    pub fn explorer_move_up(&mut self) {
        if self.explorer_index > 0 {
            self.explorer_index -= 1;
            self.explorer_list_state.select(Some(self.explorer_index));
        }
    }

    pub fn explorer_move_down(&mut self) {
        if self.explorer_index < self.dir_entries.len().saturating_sub(1) {
            self.explorer_index += 1;
            self.explorer_list_state.select(Some(self.explorer_index));
        }
    }

    /// Toggle a file in the multi-select list
    pub fn toggle_file_selection(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let selected = self.dir_entries[self.explorer_index].clone();
        if selected == Path::new("..") || selected.is_dir() || !is_video_file(&selected) {
            return;
        }

        if let Some(pos) = self.selected_files.iter().position(|f| f == &selected) {
            self.selected_files.remove(pos);
        } else {
            self.selected_files.push(selected);
        }
    }

    /// Confirm the queued files from the confirmation screen and start analysis
    pub fn confirm_queued_files(&mut self) {
        self.selected_files.clear();
        self.analyze_jobs();
    }

    /// Navigate back from file confirm to the explorer
    pub fn cancel_file_confirm(&mut self) {
        if self.selection_mode == SelectionMode::File {
            self.selected_files = self.queue.jobs.iter().map(|j| j.path.clone()).collect();
        }
        self.queue.jobs.clear();
        let select_folder = self.selection_mode == SelectionMode::Folder;
        self.current_screen = Screen::FileExplorer { select_folder };
    }

    pub fn enter_directory(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let selected = self.dir_entries[self.explorer_index].clone();

        if selected == Path::new("..") {
            if let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                self.refresh_dir_entries();
            }
        } else if selected.is_dir() {
            self.current_dir = selected;
            self.refresh_dir_entries();
        }
    }

    pub fn select_explorer_entry(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let selected = self.dir_entries[self.explorer_index].clone();

        match self.selection_mode {
            SelectionMode::File => {
                if selected == Path::new("..") || selected.is_dir() {
                    self.enter_directory();
                } else if is_video_file(&selected) {
                    if self.selected_files.is_empty() {
                        // Single file
                        self.queue.reset();
                        self.queue.jobs.push(EncodingJob::new(selected));
                        self.analyze_jobs();
                    } else {
                        // Multi-file — include current file and go to confirmation
                        if !self.selected_files.contains(&selected) {
                            self.selected_files.push(selected);
                        }
                        self.queue.reset();
                        for path in &self.selected_files {
                            self.queue.jobs.push(EncodingJob::new(path.clone()));
                        }
                        self.navigate_to_file_confirm();
                    }
                }
            }
            SelectionMode::Folder | SelectionMode::FolderRecursive => {
                if selected == Path::new("..") || !selected.is_dir() {
                    self.enter_directory();
                } else {
                    let recursive = self.selection_mode == SelectionMode::FolderRecursive;
                    self.scan_folder(&selected, recursive);
                    if self.queue.jobs.is_empty() {
                        let msg =
                            crate::i18n::t(self.config.language, crate::i18n::Msg::NoVideoFiles);
                        self.set_message(msg);
                    } else if self.queue.jobs.len() == 1 {
                        // Single file in folder — proceed directly
                        self.analyze_jobs();
                    } else {
                        // Multiple files — show confirmation
                        self.navigate_to_file_confirm();
                    }
                }
            }
        }
    }

    pub fn scan_folder(&mut self, folder: &Path, recursive: bool) {
        self.queue.reset();

        if recursive {
            let mut paths: Vec<PathBuf> = Vec::new();
            collect_video_files(folder, &mut paths);
            paths.sort();
            for path in paths {
                self.queue.jobs.push(EncodingJob::new(path));
            }
        } else if let Ok(entries) = std::fs::read_dir(folder) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| is_video_file(p))
                .collect();

            paths.sort();

            for path in paths {
                self.queue.jobs.push(EncodingJob::new(path));
            }
        }
    }

    fn analyze_jobs(&mut self) {
        // Pre-validate paths
        let mut paths: Vec<Result<String, AppError>> = Vec::new();
        for job in &mut self.queue.jobs {
            match job.path.to_string_lossy() {
                p if job.path.to_str().is_some() => {
                    paths.push(Ok(p.into_owned()));
                    job.status = JobStatus::Analyzing;
                }
                _ => {
                    // Path has non-UTF-8 bytes
                    job.status = JobStatus::Error {
                        message: "File path contains non-UTF-8 characters".to_string(),
                    };
                    self.queue.error_count += 1;
                    paths.push(Err(AppError::Analysis(
                        "File path contains non-UTF-8 characters".to_string(),
                    )));
                }
            }
        }

        self.analysis_cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag = self.analysis_cancel_flag.clone();

        let (tx, rx) = mpsc::channel();
        self.analysis_receiver = Some(rx);

        thread::spawn(move || {
            let results: Vec<Result<AnalysisResult, AppError>> = std::thread::scope(|s| {
                // Spawn one thread per valid path
                let handles: Vec<Option<_>> = paths
                    .iter()
                    .map(|pr| match pr {
                        Ok(p) => {
                            if cancel_flag.load(Ordering::Relaxed) {
                                None // skip remaining if already cancelled
                            } else {
                                let p = p.clone();
                                let flag = cancel_flag.clone();
                                Some(s.spawn(move || {
                                    if flag.load(Ordering::Relaxed) {
                                        Err(AppError::Analysis("Cancelled".to_string()))
                                    } else {
                                        analyzer::analyze(&p)
                                    }
                                }))
                            }
                        }
                        Err(_) => None,
                    })
                    .collect();

                handles
                    .into_iter()
                    .zip(paths.iter())
                    .map(|(handle, original)| match handle {
                        Some(h) => h.join().unwrap_or_else(|_| {
                            Err(AppError::Analysis("Analysis thread panicked".to_string()))
                        }),
                        None => match original {
                            Err(e) => Err(AppError::Analysis(e.to_string())),
                            Ok(_) => Err(AppError::Analysis("Cancelled".to_string())),
                        },
                    })
                    .collect()
            });
            let _ = tx.send(results);
        });

        self.navigate_to_queue();
    }

    /// Apply completed analysis results and advance to the next screen.
    fn apply_analysis_results(&mut self, results: Vec<Result<AnalysisResult, AppError>>) {
        let output_config = self.config.output.clone();
        let track_config = self.config.tracks.clone();

        for (job, result) in self.queue.jobs.iter_mut().zip(results) {
            match result {
                Ok(analysis) => {
                    let is_av1 = is_av1_codec(&analysis.metadata.codec_name);
                    job.metadata = Some(analysis.metadata);
                    job.audio_tracks = analysis.audio_tracks;
                    job.subtitle_tracks = analysis.subtitle_tracks;
                    job.remux_only = is_av1;
                    auto_select_tracks(job, &track_config);
                    job.generate_output_path(&output_config);
                    job.status = JobStatus::AwaitingConfig;
                }
                Err(ref e) if e.to_string().contains("Cancelled") => {
                    if matches!(job.status, JobStatus::Analyzing) {
                        job.status = JobStatus::Skipped {
                            reason: "Cancelled".to_string(),
                        };
                        self.queue.skipped_count += 1;
                    }
                }
                Err(e) => {
                    job.status = JobStatus::Error {
                        message: e.to_string(),
                    };
                    self.queue.error_count += 1;
                }
            }
        }

        // Find first job awaiting config
        self.queue.config_job_index = self
            .queue
            .jobs
            .iter()
            .position(|j| matches!(j.status, JobStatus::AwaitingConfig))
            .unwrap_or(0);

        if self
            .queue
            .jobs
            .iter()
            .any(|j| matches!(j.status, JobStatus::AwaitingConfig))
        {
            self.navigate_to_track_config();
        } else {
            self.navigate_to_finish();
        }
    }

    /// Poll the analysis channel; called every frame from the main loop.
    pub fn process_analysis_messages(&mut self) {
        let results = if let Some(ref rx) = self.analysis_receiver {
            rx.try_recv().ok()
        } else {
            return;
        };

        if let Some(results) = results {
            self.analysis_receiver = None;
            self.apply_analysis_results(results);
        }
    }

    /// Cancel an in-progress analysis and return to the home screen.
    pub fn cancel_analysis(&mut self) {
        // Signal the analysis thread to stop spawning new ffprobe calls
        self.analysis_cancel_flag.store(true, Ordering::Relaxed);
        self.analysis_receiver = None;
        self.queue.reset();
        self.navigate_to_home();
    }

    // Track configuration

    pub fn current_config_job(&self) -> Option<&EncodingJob> {
        self.queue.jobs.get(self.queue.config_job_index)
    }

    pub fn current_config_job_mut(&mut self) -> Option<&mut EncodingJob> {
        self.queue.jobs.get_mut(self.queue.config_job_index)
    }

    pub fn confirm_track_config(&mut self) {
        if let Some(job) = self.queue.jobs.get_mut(self.queue.config_job_index) {
            job.status = JobStatus::Ready;
        }

        // Find next job awaiting config
        let next_index = self
            .queue
            .jobs
            .iter()
            .skip(self.queue.config_job_index + 1)
            .position(|j| matches!(j.status, JobStatus::AwaitingConfig))
            .map(|i| i + self.queue.config_job_index + 1);

        if let Some(idx) = next_index {
            self.queue.config_job_index = idx;
            self.reset_track_config_cursor();
        } else {
            self.start_encoding();
        }
    }

    /// Abandon the whole batch and return to Home, discarding the queue.
    pub fn cancel_track_config(&mut self) {
        self.queue.reset();
        self.navigate_to_home();
    }

    /// Move to the previous (`forward = false`) or next (`forward = true`)
    /// configurable job in the batch, skipping jobs that never get a track
    /// config step (errored or skipped during analysis). No-op at either end.
    pub fn step_track_config_job(&mut self, forward: bool) {
        let len = self.queue.jobs.len();
        if len == 0 {
            return;
        }

        let mut idx = self.queue.config_job_index;
        loop {
            let next = if forward {
                idx.checked_add(1)
            } else {
                idx.checked_sub(1)
            };
            let Some(next) = next.filter(|&i| i < len) else {
                return;
            };
            idx = next;

            let configurable = self.queue.jobs.get(idx).is_some_and(|j| {
                !matches!(
                    j.status,
                    JobStatus::Error { .. } | JobStatus::Skipped { .. }
                )
            });
            if configurable {
                self.queue.config_job_index = idx;
                self.reset_track_config_cursor();
                return;
            }
        }
    }

    // Encoding

    pub fn start_encoding(&mut self) {
        info!("Starting encoding process");
        self.navigate_to_queue();
        self.encoding_active = true;
        self.queue.current_job_index = 0;
        self.cancel_flag = Arc::new(AtomicBool::new(false));

        let (tx, rx) = mpsc::channel();
        self.progress_receiver = Some(rx);

        let output_config = self.config.output.clone();

        // Collect jobs to encode
        let worker_jobs: Vec<WorkerJob> = self
            .queue
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| matches!(j.status, JobStatus::Ready))
            .filter_map(|(i, j)| {
                let metadata = j.metadata.clone()?;
                let output = j.output_path.clone().unwrap_or_else(|| {
                    let stem = j.path.file_stem().unwrap_or_default().to_string_lossy();
                    let parent = j.path.parent().unwrap_or(std::path::Path::new("."));
                    parent.join(format!(
                        "{}{}.{}",
                        stem, output_config.suffix, output_config.container
                    ))
                });
                Some(WorkerJob {
                    index: i,
                    input: j.path.clone(),
                    output,
                    metadata,
                    tracks: j.track_selection.clone(),
                    remux_only: j.remux_only,
                })
            })
            .collect();

        info!("Jobs to encode: {}", worker_jobs.len());

        self.queue.start_time = Some(std::time::Instant::now());
        self.queue.total_jobs_to_encode = worker_jobs.len();

        // Mark jobs as pending
        for wj in &worker_jobs {
            if let Some(j) = self.queue.jobs.get_mut(wj.index) {
                j.status = JobStatus::Pending;
            }
        }

        let cancel_flag = self.cancel_flag.clone();
        let config = self.config.clone();

        thread::spawn(move || {
            run_worker(worker_jobs, &config, &cancel_flag, &tx);
        });
    }

    pub fn cancel_encoding(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    #[allow(clippy::too_many_lines)]
    pub fn process_progress_messages(&mut self) {
        let messages: Vec<WorkerMessage> = if let Some(ref rx) = self.progress_receiver {
            let mut msgs = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
            msgs
        } else {
            return;
        };

        let mut should_finish = false;

        for msg in messages {
            match msg {
                WorkerMessage::Progress(idx, progress) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::Encoding { progress };
                        self.queue.current_job_index = idx;
                    }
                }
                WorkerMessage::Done(idx) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::Done;
                        self.queue.converted_count += 1;
                        self.queue.encoding_progress_done += 1;
                    }
                    if self.queue.all_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                WorkerMessage::DoneWithVmaf(idx, score) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::DoneWithVmaf { score };
                        self.queue.converted_count += 1;
                        self.queue.encoding_progress_done += 1;
                    }
                    if self.queue.all_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                WorkerMessage::Error(idx, msg) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::Error { message: msg };
                        self.queue.error_count += 1;
                        self.queue.encoding_progress_done += 1;
                    }
                    if self.queue.all_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                WorkerMessage::QualityWarning(idx, vmaf, threshold) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::QualityWarning { vmaf, threshold };
                        self.queue.converted_count += 1;
                        self.queue.encoding_progress_done += 1;
                    }
                    if self.queue.all_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                WorkerMessage::Verifying(idx) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::Verifying;
                    }
                }
                WorkerMessage::DoneVmafFailed(idx, reason) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::DoneVmafFailed { reason };
                        self.queue.converted_count += 1;
                        self.queue.encoding_progress_done += 1;
                    }
                    if self.queue.all_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                WorkerMessage::SourceDeleted(idx) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.source_deleted = true;
                    }
                }
                WorkerMessage::SourceKeptLowVmaf(idx, vmaf) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.source_kept_vmaf = Some(vmaf);
                    }
                }
                WorkerMessage::Cancelled => {
                    for job in &mut self.queue.jobs {
                        if matches!(
                            job.status,
                            JobStatus::Pending
                                | JobStatus::Ready
                                | JobStatus::Encoding { .. }
                                | JobStatus::Verifying
                        ) {
                            job.status = JobStatus::Skipped {
                                reason: "Cancelled".to_string(),
                            };
                            self.queue.skipped_count += 1;
                        }
                    }
                    self.encoding_active = false;
                    should_finish = true;
                }
            }
        }

        if should_finish {
            self.queue.end_time = Some(std::time::Instant::now());
            self.navigate_to_finish();
        }
    }

    pub fn reset(&mut self) {
        self.queue.reset();
        self.encoding_active = false;
        self.selected_files.clear();
        self.progress_receiver = None;
        self.navigate_to_home();
    }
}

fn collect_video_files(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect_video_files(&path, paths);
        } else if is_video_file(&path) {
            paths.push(path);
        }
    }
}

/// Select audio and subtitle tracks based on configured language preferences.
fn auto_select_tracks(job: &mut EncodingJob, config: &TrackPresetConfig) {
    // Audio tracks
    let preferred_audio: Vec<usize> = job
        .audio_tracks
        .iter()
        .filter(|t| {
            t.language.as_deref().is_some_and(|l| {
                config
                    .preferred_audio_languages
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(l))
            })
        })
        .map(|t| t.index)
        .collect();

    job.track_selection.audio_indices = if !preferred_audio.is_empty() {
        preferred_audio
    } else if config.select_all_fallback || config.preferred_audio_languages.is_empty() {
        job.audio_tracks.iter().map(|t| t.index).collect()
    } else {
        job.audio_tracks
            .first()
            .map(|t| vec![t.index])
            .unwrap_or_default()
    };

    // Subtitle tracks
    let preferred_subs: Vec<usize> = job
        .subtitle_tracks
        .iter()
        .filter(|t| {
            t.language.as_deref().is_some_and(|l| {
                config
                    .preferred_subtitle_languages
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(l))
            })
        })
        .map(|t| t.index)
        .collect();

    job.track_selection.subtitle_indices = if !preferred_subs.is_empty() {
        preferred_subs
    } else if config.select_all_fallback || config.preferred_subtitle_languages.is_empty() {
        job.subtitle_tracks.iter().map(|t| t.index).collect()
    } else {
        Vec::new()
    };
}
