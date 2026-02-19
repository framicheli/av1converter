use crate::analyzer::{self, is_av1_codec};
use crate::config::AppConfig;
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
use tracing::info;

/// Application screens
#[derive(Debug, Clone, PartialEq)]
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
}

/// Track configuration focus
#[derive(Debug, Clone, PartialEq)]
pub enum TrackFocus {
    Audio,
    Subtitle,
    Confirm,
}

/// Confirmation dialog action
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    CancelEncoding,
    ExitApp,
}

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
    pub recursive_scan: bool,

    // Queue state (replaces Vec<VideoFile>)
    pub queue: QueueState,

    // Track config
    pub track_focus: TrackFocus,
    pub audio_cursor: usize,
    pub subtitle_cursor: usize,
    pub audio_list_state: ListState,
    pub subtitle_list_state: ListState,

    // Home menu
    pub home_index: usize,
    pub home_menu_count: usize,

    // Multi-file selection
    pub selected_files: Vec<PathBuf>,
    pub file_confirm_scroll: usize,

    // Encoding
    pub encoding_active: bool,
    pub progress_receiver: Option<Receiver<WorkerMessage>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub delete_source: bool,

    // Configuration
    pub config: AppConfig,
    pub deps: bool,

    // UI state
    pub message: Option<String>,
    pub confirm_dialog: Option<ConfirmAction>,
    pub confirm_selection: bool,

    // Config screen state
    pub config_scroll: usize,
    pub config_selected: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let mut audio_list_state = ListState::default();
        audio_list_state.select(Some(0));
        let mut subtitle_list_state = ListState::default();
        subtitle_list_state.select(Some(0));

        let config = AppConfig::load();
        let deps = DependencyStatus::check().unwrap_or(false);

        info!("Using encoder: {}", config.encoder);

        Self {
            current_screen: Screen::Home,
            should_quit: false,
            selection_mode: SelectionMode::File,
            current_dir,
            dir_entries: Vec::new(),
            explorer_index: 0,
            explorer_list_state: list_state,
            recursive_scan: false,
            queue: QueueState::new(),
            track_focus: TrackFocus::Audio,
            audio_cursor: 0,
            subtitle_cursor: 0,
            audio_list_state,
            subtitle_list_state,
            home_index: 0,
            home_menu_count: 5,
            selected_files: Vec::new(),
            file_confirm_scroll: 0,
            encoding_active: false,
            progress_receiver: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            delete_source: false,
            config,
            deps,
            message: None,
            confirm_dialog: None,
            confirm_selection: false,
            config_scroll: 0,
            config_selected: 0,
        }
    }

    // Message handling

    pub fn set_message(&mut self, msg: &str) {
        self.message = Some(msg.to_string());
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    // Navigation

    pub fn navigate_to_home(&mut self) {
        self.current_screen = Screen::Home;
        self.home_index = 0;
        self.selected_files.clear();
    }

    pub fn navigate_to_explorer(&mut self, select_folder: bool, recursive: bool) {
        self.selection_mode = if select_folder {
            SelectionMode::Folder
        } else {
            SelectionMode::File
        };
        self.recursive_scan = recursive;
        self.refresh_dir_entries();
        self.current_screen = Screen::FileExplorer { select_folder };
    }

    pub fn navigate_to_track_config(&mut self) {
        self.track_focus = TrackFocus::Audio;
        self.audio_cursor = 0;
        self.subtitle_cursor = 0;
        self.current_screen = Screen::TrackConfig;
    }

    pub fn navigate_to_queue(&mut self) {
        self.current_screen = Screen::Queue;
    }

    pub fn navigate_to_finish(&mut self) {
        // Update output sizes for completed jobs
        for job in &mut self.queue.jobs {
            if matches!(
                job.status,
                JobStatus::Done | JobStatus::DoneWithVmaf { .. } | JobStatus::QualityWarning { .. }
            ) && let Some(ref output_path) = job.output_path
            {
                job.output_size = std::fs::metadata(output_path).ok().map(|m| m.len());
            }
        }
        self.current_screen = Screen::Finish;
    }

    pub fn navigate_to_configuration(&mut self) {
        self.config_scroll = 0;
        self.config_selected = 0;
        self.current_screen = Screen::Configuration;
    }

    pub fn navigate_to_file_confirm(&mut self) {
        self.file_confirm_scroll = 0;
        self.current_screen = Screen::FileConfirm;
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
                .filter_map(|e| e.ok())
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
                        // Single file — proceed directly (backward compatible)
                        self.queue.jobs.clear();
                        self.queue.jobs.push(EncodingJob::new(selected));
                        self.analyze_jobs();
                    } else {
                        // Multi-file — include current file and go to confirmation
                        if !self.selected_files.contains(&selected) {
                            self.selected_files.push(selected);
                        }
                        self.queue.jobs.clear();
                        for path in &self.selected_files {
                            self.queue.jobs.push(EncodingJob::new(path.clone()));
                        }
                        self.navigate_to_file_confirm();
                    }
                }
            }
            SelectionMode::Folder => {
                if selected == Path::new("..") || !selected.is_dir() {
                    self.enter_directory();
                } else {
                    self.scan_folder(&selected, self.recursive_scan);
                    if self.queue.jobs.is_empty() {
                        self.set_message("No video files found in this folder");
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

    pub fn scan_folder(&mut self, folder: &PathBuf, recursive: bool) {
        self.queue.jobs.clear();

        if recursive {
            let mut paths: Vec<PathBuf> = Vec::new();
            collect_video_files(folder, &mut paths);
            paths.sort();
            for path in paths {
                self.queue.jobs.push(EncodingJob::new(path));
            }
        } else if let Ok(entries) = std::fs::read_dir(folder) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
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
        let suffix = self.config.output.suffix.clone();
        let container = self.config.output.container.clone();

        for job in &mut self.queue.jobs {
            job.status = JobStatus::Analyzing;

            match analyzer::analyze(job.path.to_str().unwrap_or("")) {
                Ok(analysis) => {
                    // Check if already AV1 - skip
                    if is_av1_codec(&analysis.metadata.codec_name) {
                        job.status = JobStatus::Skipped {
                            reason: "Already AV1".to_string(),
                        };
                        self.queue.skipped_count += 1;
                        continue;
                    }

                    job.metadata = Some(analysis.metadata);
                    job.audio_tracks = analysis.audio_tracks;
                    job.subtitle_tracks = analysis.subtitle_tracks;
                    job.select_all_tracks();
                    job.generate_output_path(&suffix, &container);
                    job.status = JobStatus::AwaitingConfig;
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
            self.track_focus = TrackFocus::Audio;
            self.audio_cursor = 0;
            self.subtitle_cursor = 0;
        } else {
            self.start_encoding();
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

        // Collect jobs to encode
        let delete_source = self.delete_source;
        let worker_jobs: Vec<WorkerJob> = self
            .queue
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| matches!(j.status, JobStatus::Ready))
            .filter_map(|(i, j)| {
                let metadata = j.metadata.clone()?;
                Some(WorkerJob {
                    index: i,
                    input: j.path.clone(),
                    output: j.output_path.clone().unwrap_or_else(|| j.path.clone()),
                    metadata,
                    tracks: j.track_selection.clone(),
                    delete_source,
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
            run_worker(worker_jobs, config, cancel_flag, tx);
        });
    }

    pub fn cancel_encoding(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

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
                        if matches!(job.status, JobStatus::Encoding { .. }) {
                            job.status = JobStatus::Skipped {
                                reason: "Cancelled".to_string(),
                            };
                        }
                    }
                    self.encoding_active = false;
                    should_finish = true;
                }
            }
        }

        if should_finish {
            self.navigate_to_finish();
        }
    }

    pub fn reset(&mut self) {
        self.queue.reset();
        self.encoding_active = false;
        self.delete_source = false;
        self.selected_files.clear();
        self.progress_receiver = None;
        self.navigate_to_home();
    }
}

fn collect_video_files(dir: &PathBuf, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_video_files(&path, paths);
        } else if is_video_file(&path) {
            paths.push(path);
        }
    }
}
