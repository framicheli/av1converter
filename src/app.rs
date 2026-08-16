use crate::analyzer::{self, AnalysisResult, DvMode, HdrType, is_av1_codec};
use crate::config::{AppConfig, Encoder};
use crate::disc::worker::DiscEvent;
use crate::disc::{DiscDrive, DiscSource, DiscTitle};
use crate::error::AppError;
use crate::queue::{
    EncodingJob, JobStatus, QueueState, WorkerJob, WorkerMessage, auto_select_tracks,
    collect_video_files, is_video_file, make_output_paths_unique, run_worker,
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
    DiscDrives,
    DiscTitles,
    TrackConfig,
    Queue,
    Finish,
    Configuration,
}

/// What the title screen is showing while a disc is being read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscState {
    Scanning,
    Ready,
    /// Already translated, ready to render.
    Failed(String),
}

/// File selection mode
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionMode {
    File,
    Folder,
    FolderRecursive,
    /// Picking a ripped disc folder or an ISO image instead of a video file.
    DiscFolder,
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
    CancelAnalysis,
}

pub const HOME_MENU: &[&str] = &[
    "Open Video File",
    "Open Folder",
    "Open Folder (Recursive)",
    "Rip DVD / Blu-ray",
    "Configuration",
    "Quit",
];

/// Main application state
// Flat screen-state flags, read one at a time.
#[allow(clippy::struct_excessive_bools)]
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

    // Disc ripping
    pub disc_drives: Vec<DiscDrive>,
    pub disc_drive_cursor: usize,
    pub disc_drive_list_state: ListState,
    pub disc_titles: Vec<DiscTitle>,
    pub disc_cursor: usize,
    pub disc_list_state: ListState,
    /// Title ids marked with Space, in the order they were marked.
    pub disc_selected: Vec<u32>,
    pub disc_state: DiscState,
    pub disc_receiver: Option<Receiver<DiscEvent>>,
    pub disc_cancel_flag: Arc<AtomicBool>,
    /// What the current scan or rip is reading from.
    pub disc_source: Option<DiscSource>,

    // Encoding
    pub encoding_active: bool,
    pub progress_receiver: Option<Receiver<WorkerMessage>>,
    pub cancel_flag: Arc<AtomicBool>,

    // Background analysis. Results are keyed by job index: a title that has
    // just finished ripping joins the same channel while others still probe.
    pub analysis_receiver: Option<Receiver<(usize, Result<AnalysisResult, AppError>)>>,
    analysis_sender: Option<mpsc::Sender<(usize, Result<AnalysisResult, AppError>)>>,
    /// Probes handed out and not yet applied.
    analysis_outstanding: usize,
    /// Ask the analysis thread to stop spawning new ffprobe calls
    pub analysis_cancel_flag: Arc<AtomicBool>,

    // Configuration
    pub config: AppConfig,
    /// ffmpeg and ffprobe are on PATH — without these nothing works
    pub deps: bool,
    /// This `FFmpeg` build has libvmaf, which only VMAF verification needs
    pub vmaf_deps: bool,
    /// Whether this `FFmpeg` build can encode Opus
    pub opus_deps: bool,

    // UI state
    pub message: Option<String>,
    pub message_expiry: Option<Instant>,
    pub confirm_dialog: Option<(ConfirmAction, bool)>,
    /// Dolby Vision mode dialog: selected option (0 = keep DV, 1 = HDR10)
    pub dv_dialog: Option<usize>,

    // Config screen state
    pub config_selected: usize,
    pub config_edit_buffer: Option<String>,
    pub config_snapshot: Option<AppConfig>,

    // Finish screen
    pub finish_cursor: usize,
    pub finish_list_state: ListState,
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
        let mut finish_list_state = ListState::default();
        finish_list_state.select(Some(0));
        let mut disc_drive_list_state = ListState::default();
        disc_drive_list_state.select(Some(0));
        let mut disc_list_state = ListState::default();
        disc_list_state.select(Some(0));

        let config = AppConfig::load();
        let deps = DependencyStatus::check();
        let vmaf_deps = DependencyStatus::vmaf_available();
        let opus_deps = DependencyStatus::libopus_available();

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
            disc_drives: Vec::new(),
            disc_drive_cursor: 0,
            disc_drive_list_state,
            disc_titles: Vec::new(),
            disc_cursor: 0,
            disc_list_state,
            disc_selected: Vec::new(),
            disc_state: DiscState::Ready,
            disc_receiver: None,
            disc_cancel_flag: Arc::new(AtomicBool::new(false)),
            disc_source: None,
            encoding_active: false,
            progress_receiver: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            analysis_receiver: None,
            analysis_sender: None,
            analysis_outstanding: 0,
            analysis_cancel_flag: Arc::new(AtomicBool::new(false)),
            config,
            deps,
            vmaf_deps,
            opus_deps,
            message: None,
            message_expiry: None,
            confirm_dialog: None,
            dv_dialog: None,
            config_selected: 0,
            config_edit_buffer: None,
            config_snapshot: None,
            finish_cursor: 0,
            finish_list_state,
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

    // Dolby Vision dialog

    /// Open the DV-mode dialog if the job at `config_job_index` is a Dolby
    /// Vision source that still needs a decision. On hardware encoders, which
    /// cannot write the DV RPU, the job resolves to HDR10 without a dialog.
    pub fn maybe_open_dv_dialog(&mut self) {
        self.dv_dialog = None;

        let Some(job) = self.current_config_job() else {
            return;
        };
        let Some(meta) = job.metadata.as_ref() else {
            return;
        };
        if job.remux_only || job.dv_mode.is_some() || meta.hdr_type != HdrType::DolbyVision {
            return;
        }

        if self.config.encoder == Encoder::SvtAv1 {
            let recommended = DvMode::recommended_for(meta.dv_profile);
            self.dv_dialog = Some(dv_mode_index(recommended));
        } else if let Some(job) = self.current_config_job_mut() {
            job.dv_mode = Some(DvMode::ToHdr10);
        }
    }

    /// Re-open the DV-mode dialog from the track config screen ('d' key).
    pub fn reopen_dv_dialog(&mut self) {
        let Some(meta) = self.current_config_job().and_then(|j| j.metadata.as_ref()) else {
            return;
        };
        if meta.hdr_type != HdrType::DolbyVision {
            return;
        }
        if self.config.encoder != Encoder::SvtAv1 {
            let msg = crate::i18n::t(self.config.language, crate::i18n::Msg::DvRequiresSvt);
            self.set_timed_message(msg, 3);
            return;
        }
        let current = self
            .current_config_job()
            .and_then(|j| j.dv_mode)
            .unwrap_or_else(|| DvMode::recommended_for(meta.dv_profile));
        self.dv_dialog = Some(dv_mode_index(current));
    }

    /// Apply the selected DV mode to the current job and close the dialog.
    pub fn confirm_dv_dialog(&mut self) {
        if let Some(sel) = self.dv_dialog.take()
            && let Some(job) = self.current_config_job_mut()
        {
            job.dv_mode = Some(if sel == 0 {
                DvMode::KeepDolbyVision
            } else {
                DvMode::ToHdr10
            });
        }
    }

    /// Close the dialog with the recommended default (Esc).
    pub fn dismiss_dv_dialog(&mut self) {
        if self.dv_dialog.take().is_some() {
            let profile = self
                .current_config_job()
                .and_then(|j| j.metadata.as_ref())
                .and_then(|m| m.dv_profile);
            if let Some(job) = self.current_config_job_mut() {
                job.dv_mode.get_or_insert(DvMode::recommended_for(profile));
            }
        }
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
        self.maybe_open_dv_dialog();
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
        self.finish_cursor = 0;
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

    /// Move the Finish screen results-list cursor up (`forward = false`) or
    /// down (`forward = true`), clamped within the job list bounds.
    pub fn finish_move_cursor(&mut self, forward: bool) {
        if forward {
            if self.finish_cursor < self.queue.jobs.len().saturating_sub(1) {
                self.finish_cursor += 1;
            }
        } else if self.finish_cursor > 0 {
            self.finish_cursor -= 1;
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
            SelectionMode::DiscFolder => {
                if selected == Path::new("..") || !selected.is_dir() {
                    self.enter_directory();
                } else {
                    self.scan_disc_folder(&selected);
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

        let mut paths: Vec<PathBuf> = Vec::new();
        if recursive {
            collect_video_files(folder, &mut paths);
        } else if let Ok(entries) = std::fs::read_dir(folder) {
            paths.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| is_video_file(p)),
            );
        }

        paths.sort();
        for path in paths {
            self.queue.jobs.push(EncodingJob::new(path));
        }
    }

    fn analyze_jobs(&mut self) {
        self.analysis_cancel_flag = Arc::new(AtomicBool::new(false));
        let indices: Vec<usize> = (0..self.queue.jobs.len()).collect();
        self.analyze_indices(&indices);
        self.navigate_to_queue();
    }

    /// Probe the jobs at `indices` on a worker thread. Probes already running
    /// keep going: a title that has just finished ripping joins them rather
    /// than waiting for a round to end.
    fn analyze_indices(&mut self, indices: &[usize]) {
        let mut work: Vec<(usize, String)> = Vec::new();
        for &index in indices {
            let Some(job) = self.queue.jobs.get_mut(index) else {
                continue;
            };
            // A job that already failed or was skipped has nothing to probe,
            // and keeps the status it reached.
            if job.status.is_terminal() {
                continue;
            }
            if let Some(path) = job.path.to_str() {
                work.push((index, path.to_string()));
                job.status = JobStatus::Analyzing;
            } else {
                job.status = JobStatus::Error {
                    message: "File path contains non-UTF-8 characters".to_string(),
                };
                self.queue.error_count += 1;
            }
        }
        if work.is_empty() {
            return;
        }

        if self.analysis_receiver.is_none() {
            let (tx, rx) = mpsc::channel();
            self.analysis_receiver = Some(rx);
            self.analysis_sender = Some(tx);
        }
        let Some(tx) = self.analysis_sender.clone() else {
            return;
        };
        let cancel_flag = self.analysis_cancel_flag.clone();
        self.analysis_outstanding += work.len();

        thread::spawn(move || {
            let paths: Vec<Result<String, AppError>> =
                work.iter().map(|(_, path)| Ok(path.clone())).collect();
            // A panic here would otherwise leave the caller counting probes
            // that never arrive.
            let results = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                analyze_batch(&paths, &cancel_flag)
            }))
            .unwrap_or_else(|_| {
                work.iter()
                    .map(|_| Err(AppError::Analysis("Analysis thread panicked".to_string())))
                    .collect()
            });
            for ((index, _), result) in work.into_iter().zip(results) {
                let _ = tx.send((index, result));
            }
        });
    }

    /// Apply one completed probe to its job.
    fn apply_analysis_result(&mut self, index: usize, result: Result<AnalysisResult, AppError>) {
        let output_config = self.config.output.clone();
        let track_config = self.config.tracks.clone();
        let audio_config = self.config.audio.clone();
        let Some(job) = self.queue.jobs.get_mut(index) else {
            return;
        };

        match result {
            Ok(analysis) => {
                let is_av1 = is_av1_codec(&analysis.metadata.codec_name);
                job.source_size = Some(analysis.source_identity.size_bytes());
                job.source_identity = Some(analysis.source_identity);
                job.metadata = Some(analysis.metadata);
                job.audio_tracks = analysis.audio_tracks;
                job.subtitle_tracks = analysis.subtitle_tracks;
                job.remux_only = is_av1;
                auto_select_tracks(job, &track_config, &audio_config);
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

    /// Every probe handed out has come back: settle the queue and move on.
    fn finish_analysis_round(&mut self) {
        self.analysis_receiver = None;
        self.analysis_sender = None;
        make_output_paths_unique(&mut self.queue.jobs);

        let next_to_configure = self
            .queue
            .jobs
            .iter()
            .position(|j| matches!(j.status, JobStatus::AwaitingConfig));
        self.queue.config_job_index = next_to_configure.unwrap_or(0);

        if next_to_configure.is_some() {
            // An encode already running keeps the screen; the job waits on the
            // queue for the user to open it.
            if !self.encoding_active {
                self.navigate_to_track_config();
            }
        } else if !self.encoding_active && self.disc_receiver.is_none() {
            // A rip still running has more titles to add to this queue.
            self.navigate_to_finish();
        }
    }

    /// Poll the analysis channel; called every frame from the main loop.
    pub fn process_analysis_messages(&mut self) {
        let mut results = Vec::new();
        if let Some(ref rx) = self.analysis_receiver {
            while let Ok(result) = rx.try_recv() {
                results.push(result);
            }
        } else {
            return;
        }
        if results.is_empty() {
            return;
        }

        for (index, result) in results {
            self.analysis_outstanding = self.analysis_outstanding.saturating_sub(1);
            self.apply_analysis_result(index, result);
        }
        if self.analysis_outstanding == 0 {
            self.finish_analysis_round();
        }
    }

    /// Cancel an in-progress analysis and return to the home screen.
    pub fn cancel_analysis(&mut self) {
        // Signal the analysis thread to stop spawning new ffprobe calls
        self.analysis_cancel_flag.store(true, Ordering::Relaxed);
        self.analysis_receiver = None;
        self.analysis_sender = None;
        self.analysis_outstanding = 0;
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
        } else if self.encoding_active {
            // A session is already running with its job list fixed; this one
            // joins the next session, started when that one ends.
            self.navigate_to_queue();
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
        let audio_config = self.config.audio.clone();

        // Collect jobs to encode
        let worker_jobs: Vec<WorkerJob> = self
            .queue
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| matches!(j.status, JobStatus::Ready))
            .filter_map(|(i, j)| {
                let metadata = j.metadata.clone()?;
                let source_identity = j.source_identity.clone()?;
                // A ripped file has no next-to-the-source fallback: that is the
                // staging directory.
                let output = if j.temporary {
                    j.output_path.clone()?
                } else {
                    j.output_path.clone().unwrap_or_else(|| {
                        let stem = j.path.file_stem().unwrap_or_default().to_string_lossy();
                        let parent = j.path.parent().unwrap_or(std::path::Path::new("."));
                        parent.join(format!(
                            "{}{}.{}",
                            stem, output_config.suffix, output_config.container
                        ))
                    })
                };
                let selected_subs = crate::tracks::selected_subtitles(
                    &j.subtitle_tracks,
                    &j.track_selection.subtitle_indices,
                );
                Some(WorkerJob {
                    index: i,
                    subtitle_codecs: crate::tracks::subtitle_codecs_for(&output, &selected_subs),
                    input: j.path.clone(),
                    output,
                    source_identity,
                    metadata,
                    tracks: j.track_selection.resolve(&j.audio_tracks, &audio_config),
                    dv_mode: j.dv_mode.unwrap_or_default(),
                    remux_only: j.remux_only,
                })
            })
            .collect();

        info!("Jobs to encode: {}", worker_jobs.len());

        self.queue.start_time = Some(std::time::Instant::now());
        self.queue.total_jobs_to_encode = worker_jobs.len();
        self.queue.encoding_progress_done = 0;

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
        // A queue can hold a rip as well as an encode.
        self.disc_cancel_flag.store(true, Ordering::Relaxed);
    }

    // Disc ripping

    /// Home → drive selection. Listing drives takes a second or two, so it runs
    /// here rather than on a thread; a title scan takes minutes and does not.
    pub fn start_disc_flow(&mut self) {
        let lang = self.config.language;
        let bin = match crate::disc::find_makemkvcon(&self.config) {
            Ok(bin) => bin,
            Err(e) => {
                self.set_timed_message(&e.message(lang), 8);
                return;
            }
        };

        self.disc_cancel_flag = Arc::new(AtomicBool::new(false));
        self.disc_drives = match crate::disc::list_drives(&bin, &self.disc_cancel_flag) {
            Ok(drives) => drives,
            // A machine with no drive still reaches the folder row.
            Err(crate::disc::DiscError::NoDrive) => Vec::new(),
            Err(e) => {
                self.set_timed_message(&e.message(lang), 8);
                return;
            }
        };
        self.disc_drive_cursor = 0;
        self.disc_drive_list_state.select(Some(0));
        self.current_screen = Screen::DiscDrives;
    }

    /// Start scanning the drive at `index`, or open the file explorer for the
    /// folder row that follows the last drive.
    pub fn scan_disc(&mut self, index: usize) {
        if index == self.disc_drives.len() {
            self.selection_mode = SelectionMode::DiscFolder;
            self.refresh_dir_entries();
            self.current_screen = Screen::FileExplorer {
                select_folder: true,
            };
            return;
        }
        let Some(drive) = self.disc_drives.get(index).cloned() else {
            return;
        };
        self.begin_disc_scan(DiscSource::Drive(drive));
    }

    /// Scan a ripped disc folder or ISO image picked in the explorer.
    pub fn scan_disc_folder(&mut self, path: &Path) {
        match DiscSource::folder(path) {
            Ok(source) => self.begin_disc_scan(source),
            Err(e) => self.set_timed_message(&e.message(self.config.language), 8),
        }
    }

    /// Start scanning `source` and show the title screen.
    ///
    /// One disc, one run: a scan or rip already going is left alone.
    fn begin_disc_scan(&mut self, source: DiscSource) {
        let lang = self.config.language;
        if self.disc_receiver.is_some() {
            return;
        }
        // A missing binary is reported as the title screen's failure state.
        let bin = match crate::disc::find_makemkvcon(&self.config) {
            Ok(bin) => bin,
            Err(e) => {
                self.disc_state = DiscState::Failed(e.message(lang));
                self.current_screen = Screen::DiscTitles;
                return;
            }
        };

        self.disc_source = Some(source.clone());
        self.disc_titles.clear();
        self.disc_selected.clear();
        self.disc_cursor = 0;
        self.disc_list_state.select(Some(0));
        self.disc_state = DiscState::Scanning;
        self.disc_cancel_flag = Arc::new(AtomicBool::new(false));

        let (tx, rx) = mpsc::channel();
        self.disc_receiver = Some(rx);
        crate::disc::worker::spawn_scan(bin, source, &self.disc_cancel_flag, tx);
        self.current_screen = Screen::DiscTitles;
    }

    pub fn disc_move_up(&mut self) {
        let cursor = if self.current_screen == Screen::DiscDrives {
            &mut self.disc_drive_cursor
        } else {
            &mut self.disc_cursor
        };
        *cursor = cursor.saturating_sub(1);
        self.sync_disc_list_state();
    }

    pub fn disc_move_down(&mut self) {
        let (cursor, len) = if self.current_screen == Screen::DiscDrives {
            (&mut self.disc_drive_cursor, self.disc_drives.len() + 1)
        } else {
            (&mut self.disc_cursor, self.disc_titles.len())
        };
        if *cursor + 1 < len {
            *cursor += 1;
        }
        self.sync_disc_list_state();
    }

    fn sync_disc_list_state(&mut self) {
        self.disc_drive_list_state
            .select(Some(self.disc_drive_cursor));
        self.disc_list_state.select(Some(self.disc_cursor));
    }

    /// Mark or unmark the title under the cursor.
    pub fn toggle_disc_title(&mut self) {
        let Some(title) = self.disc_titles.get(self.disc_cursor) else {
            return;
        };
        if let Some(pos) = self.disc_selected.iter().position(|id| *id == title.id) {
            self.disc_selected.remove(pos);
        } else {
            self.disc_selected.push(title.id);
        }
    }

    /// Queue every marked title and start extracting them.
    pub fn start_disc_rip(&mut self) {
        let lang = self.config.language;
        if self.disc_state != DiscState::Ready
            || self.disc_selected.is_empty()
            || self.disc_receiver.is_some()
        {
            return;
        }
        // Refused here rather than after the first forty-minute extraction.
        if let Err(e) = crate::disc::staging::require_destination(&self.config) {
            self.disc_state = DiscState::Failed(e.message(lang));
            return;
        }
        let bin = match crate::disc::find_makemkvcon(&self.config) {
            Ok(bin) => bin,
            Err(e) => {
                self.disc_state = DiscState::Failed(e.message(lang));
                return;
            }
        };
        let Some(source) = self.disc_source.clone() else {
            return;
        };

        let titles: Vec<DiscTitle> = self
            .disc_titles
            .iter()
            .filter(|title| self.disc_selected.contains(&title.id))
            .cloned()
            .collect();

        // Each title is a queue job from the start, so the rip renders in the
        // queue screen and shares its cancellation. The path is the title's
        // name until the file it extracts to is known.
        self.queue.reset();
        for title in &titles {
            let mut job = EncodingJob::new(PathBuf::from(title.name.clone()));
            job.status = JobStatus::Ripping { progress: 0.0 };
            job.temporary = true;
            self.queue.jobs.push(job);
        }

        self.disc_cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        self.disc_receiver = Some(rx);
        crate::disc::worker::spawn_rips(
            bin,
            self.config.clone(),
            source,
            titles,
            &self.disc_cancel_flag,
            tx,
        );
        self.navigate_to_queue();
    }

    /// Esc: cancel whatever is running and step back one screen.
    pub fn leave_disc_screen(&mut self) {
        self.disc_cancel_flag.store(true, Ordering::Relaxed);
        self.disc_receiver = None;
        match self.current_screen {
            Screen::DiscTitles => self.current_screen = Screen::DiscDrives,
            _ => self.navigate_to_home(),
        }
    }

    /// Poll the disc channel; called every frame from the main loop.
    pub fn process_disc_events(&mut self) {
        let lang = self.config.language;
        let mut events = Vec::new();
        let mut worker_gone = false;
        if let Some(ref rx) = self.disc_receiver {
            loop {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        worker_gone = true;
                        break;
                    }
                }
            }
        } else {
            return;
        }

        for event in events {
            match event {
                DiscEvent::TitlesFound(scan) => {
                    self.disc_titles = scan.titles;
                    self.disc_state = DiscState::Ready;
                    self.disc_receiver = None;
                }
                DiscEvent::Ripping { index, progress } => {
                    if let Some(job) = self.queue.jobs.get_mut(index) {
                        job.status = JobStatus::Ripping { progress };
                        if self.queue_cursor == self.queue.current_job_index {
                            self.queue_cursor = index;
                            self.queue_list_state.select(Some(index));
                        }
                        self.queue.current_job_index = index;
                    }
                }
                // Probing starts now, while the drive moves on to the next
                // title: that is what keeps peak disk at one rip plus one
                // encode input.
                DiscEvent::TitleReady { index, path } => {
                    if let Some(job) = self.queue.jobs.get_mut(index) {
                        job.source_size = std::fs::metadata(&path).ok().map(|m| m.len());
                        job.path = path;
                        job.status = JobStatus::Pending;
                    }
                    self.analyze_indices(&[index]);
                }
                DiscEvent::Error { index, error } => {
                    self.disc_receiver = None;
                    self.fail_disc_run(index, &error.message(lang));
                }
                DiscEvent::Cancelled => {
                    self.disc_receiver = None;
                    if self.disc_state == DiscState::Scanning {
                        self.disc_state = DiscState::Ready;
                    }
                    self.skip_remaining_rips();
                    self.settle_after_rips();
                }
                DiscEvent::Finished => {
                    self.disc_receiver = None;
                    self.settle_after_rips();
                }
            }
        }

        if worker_gone && self.disc_receiver.is_some() {
            self.disc_receiver = None;
            let message =
                crate::disc::DiscError::Failed("the run stopped unexpectedly".to_string())
                    .message(lang);
            self.fail_disc_run(0, &message);
        }
    }

    /// Report a failure where the user is looking: on the title screen while
    /// scanning, on the queue once titles are being extracted.
    fn fail_disc_run(&mut self, index: usize, message: &str) {
        if self.disc_state == DiscState::Scanning || self.current_screen == Screen::DiscTitles {
            self.disc_state = DiscState::Failed(message.to_string());
            return;
        }
        self.fail_remaining_rips(index, message);
        self.settle_after_rips();
    }

    /// Record the failure on the title that hit it, and close out the ones
    /// behind it: the worker stops at the first failure.
    fn fail_remaining_rips(&mut self, index: usize, message: &str) {
        for (position, job) in self.queue.jobs.iter_mut().enumerate() {
            if !matches!(job.status, JobStatus::Ripping { .. }) {
                continue;
            }
            if position == index {
                job.status = JobStatus::Error {
                    message: message.to_string(),
                };
                self.queue.error_count += 1;
            } else {
                job.status = JobStatus::Skipped {
                    reason: "Cancelled".to_string(),
                };
                self.queue.skipped_count += 1;
            }
        }
    }

    /// Whether any job is still waiting for its tracks to be chosen.
    pub fn has_jobs_awaiting_config(&self) -> bool {
        self.queue
            .jobs
            .iter()
            .any(|job| matches!(job.status, JobStatus::AwaitingConfig))
    }

    /// Open track configuration for the first job that still needs it.
    pub fn configure_next_job(&mut self) {
        let Some(index) = self
            .queue
            .jobs
            .iter()
            .position(|job| matches!(job.status, JobStatus::AwaitingConfig))
        else {
            return;
        };
        self.queue.config_job_index = index;
        self.navigate_to_track_config();
    }

    /// Titles that were queued but never extracted.
    fn skip_remaining_rips(&mut self) {
        for job in &mut self.queue.jobs {
            if matches!(job.status, JobStatus::Ripping { .. }) {
                job.status = JobStatus::Skipped {
                    reason: "Cancelled".to_string(),
                };
                self.queue.skipped_count += 1;
            }
        }
    }

    /// The rip run is over. Probes for the titles it produced may still be
    /// running, and they carry the queue on from here.
    fn settle_after_rips(&mut self) {
        if self.analysis_receiver.is_none() && self.current_screen != Screen::TrackConfig {
            self.navigate_to_queue();
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn process_progress_messages(&mut self) {
        let mut worker_gone = false;
        let messages: Vec<WorkerMessage> = if let Some(ref rx) = self.progress_receiver {
            let mut msgs = Vec::new();
            loop {
                match rx.try_recv() {
                    Ok(msg) => msgs.push(msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    // The worker thread is gone; nothing more is coming.
                    Err(mpsc::TryRecvError::Disconnected) => {
                        worker_gone = true;
                        break;
                    }
                }
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
                        // The cursor follows the active job until manually
                        // scrolled elsewhere.
                        if self.queue_cursor == self.queue.current_job_index {
                            self.queue_cursor = idx;
                        }
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
                            self.queue.encoding_progress_done += 1;
                        }
                    }
                    self.encoding_active = false;
                    should_finish = true;
                }
            }
        }

        // The worker is gone; close out whatever it left unfinished.
        if worker_gone && self.encoding_active {
            self.progress_receiver = None;
            for job in &mut self.queue.jobs {
                if matches!(
                    job.status,
                    JobStatus::Pending | JobStatus::Encoding { .. } | JobStatus::Verifying
                ) {
                    job.status = JobStatus::Error {
                        message: "Encoding stopped unexpectedly".to_string(),
                    };
                    self.queue.error_count += 1;
                    self.queue.encoding_progress_done += 1;
                }
            }
            self.encoding_active = false;
            should_finish = true;
        }

        crate::disc::staging::cleanup_finished(&mut self.queue.jobs);

        if should_finish {
            // A title that finished ripping while this session ran is Ready
            // but was not in its job list, so it gets a session of its own.
            // Not after a worker died: that would restart the same failure.
            let leftovers = !worker_gone
                && self
                    .queue
                    .jobs
                    .iter()
                    .any(|job| matches!(job.status, JobStatus::Ready));
            if leftovers {
                self.start_encoding();
            } else if self.disc_receiver.is_none() && self.analysis_receiver.is_none() {
                self.queue.end_time = Some(std::time::Instant::now());
                self.navigate_to_finish();
            }
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

/// Probe a batch of files, keeping results in input order. A fixed pool of
/// workers pulls from the batch, each rechecking `cancel_flag` before taking
/// the next file.
fn analyze_batch(
    paths: &[Result<String, AppError>],
    cancel_flag: &AtomicBool,
) -> Vec<Result<AnalysisResult, AppError>> {
    /// Concurrent ffprobe calls.
    const MAX_WORKERS: usize = 4;

    let slots: Vec<std::sync::Mutex<Option<Result<AnalysisResult, AppError>>>> = (0..paths.len())
        .map(|_| std::sync::Mutex::new(None))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = MAX_WORKERS.min(paths.len().max(1));

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else {
                        break;
                    };
                    let result = match path {
                        _ if cancel_flag.load(Ordering::Relaxed) => {
                            Err(AppError::Analysis("Cancelled".to_string()))
                        }
                        Ok(path) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            analyzer::analyze(path, cancel_flag)
                        }))
                        .unwrap_or_else(|_| {
                            Err(AppError::Analysis(format!("Analysis panicked on {path}")))
                        }),
                        Err(e) => Err(AppError::Analysis(e.to_string())),
                    };
                    if let Ok(mut slot) = slots[index].lock() {
                        *slot = Some(result);
                    }
                }
            });
        }
    });

    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .ok()
                .flatten()
                .unwrap_or_else(|| Err(AppError::Analysis("Analysis thread panicked".to_string())))
        })
        .collect()
}

/// Dialog option index for a DV mode (0 = keep DV, 1 = HDR10)
fn dv_mode_index(mode: DvMode) -> usize {
    match mode {
        DvMode::KeepDolbyVision => 0,
        DvMode::ToHdr10 => 1,
    }
}
