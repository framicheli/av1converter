use crate::analysis::{Resolution, analyze_full};
use crate::converter::{EncodeOptions, EncodeResult, TrackSelection, encode_video};
use crate::data::{FileStatus, VideoFile, is_video_file};
use crate::encoder::{ContentType, EncoderConfig};
use crate::vmaf::is_vmaf_available;
use ratatui::widgets::ListState;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Instant;
use tracing::{debug, info};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    FileExplorer { select_folder: bool },
    TrackConfig,
    Queue,
    Finish,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectionMode {
    File,
    Folder,
}

pub struct App {
    pub current_screen: Screen,
    pub should_quit: bool,
    pub selection_mode: SelectionMode,

    // File explorer state
    pub current_dir: PathBuf,
    pub dir_entries: Vec<PathBuf>,
    pub explorer_index: usize,
    pub explorer_list_state: ListState,

    // Video queue
    pub files: Vec<VideoFile>,
    pub current_file_index: usize,
    pub config_file_index: usize,

    // Track config state
    pub track_focus: TrackFocus,
    pub audio_cursor: usize,
    pub subtitle_cursor: usize,

    // Home menu
    pub home_index: usize,

    // Encoding
    pub encoding_active: bool,
    pub progress_receiver: Option<Receiver<ProgressMessage>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub start_time: Option<Instant>,
    pub total_files_to_encode: usize,
    pub encoder_config: EncoderConfig,

    // Stats
    pub converted_count: usize,
    pub skipped_count: usize,
    pub error_count: usize,

    // Message/notification
    pub message: Option<String>,

    // Confirmation dialog
    pub confirm_dialog: Option<ConfirmAction>,
    pub confirm_selection: bool, // true = Yes, false = No

    // Track list states for scrolling
    pub audio_list_state: ListState,
    pub subtitle_list_state: ListState,

    // VMAF availability (cached at startup)
    pub vmaf_available: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackFocus {
    Audio,
    Subtitle,
    Confirm,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    CancelEncoding,
    ExitApp,
}

pub enum ProgressMessage {
    Progress(usize, f32),
    Done(usize),
    DoneWithVmaf(usize, f64),
    Error(usize, String),
    QualityWarning(usize, f64, f64), // index, vmaf, threshold
    Cancelled,
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

        // Detect available AV1 encoders at startup
        let encoder_config = EncoderConfig::new();

        // Check VMAF availability
        let vmaf_available = is_vmaf_available();

        info!("Using encoder: {}", encoder_config.selected_encoder);
        info!("VMAF available: {}", vmaf_available);

        Self {
            current_screen: Screen::Home,
            should_quit: false,
            selection_mode: SelectionMode::File,
            current_dir: current_dir.clone(),
            dir_entries: Vec::new(),
            explorer_index: 0,
            explorer_list_state: list_state,
            files: Vec::new(),
            current_file_index: 0,
            config_file_index: 0,
            track_focus: TrackFocus::Audio,
            audio_cursor: 0,
            subtitle_cursor: 0,
            home_index: 0,
            encoding_active: false,
            progress_receiver: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            start_time: None,
            total_files_to_encode: 0,
            encoder_config,
            converted_count: 0,
            skipped_count: 0,
            error_count: 0,
            message: None,
            confirm_dialog: None,
            confirm_selection: false, // Default to "No"
            audio_list_state,
            subtitle_list_state,
            vmaf_available,
        }
    }

    pub fn set_message(&mut self, msg: &str) {
        self.message = Some(msg.to_string());
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn refresh_dir_entries(&mut self) {
        self.dir_entries.clear();

        // Add parent directory if not at root
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
                .filter(|p| {
                    // Show directories and video files
                    p.is_dir() || is_video_file(p)
                })
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

    pub fn enter_directory(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let selected = self.dir_entries[self.explorer_index].clone();

        if selected == Path::new("..") {
            // Go to parent directory
            if let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                self.refresh_dir_entries();
            }
        } else if selected.is_dir() {
            // Enter directory
            self.current_dir = selected;
            self.refresh_dir_entries();
        }
    }

    pub fn navigate_to_home(&mut self) {
        self.current_screen = Screen::Home;
        self.home_index = 0;
    }

    pub fn navigate_to_explorer(&mut self, select_folder: bool) {
        self.selection_mode = if select_folder {
            SelectionMode::Folder
        } else {
            SelectionMode::File
        };
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
        self.current_screen = Screen::Finish;
    }

    pub fn select_explorer_entry(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let selected = self.dir_entries[self.explorer_index].clone();

        match self.selection_mode {
            SelectionMode::File => {
                if selected == Path::new("..") {
                    // Go to parent directory
                    self.enter_directory();
                } else if selected.is_dir() {
                    // Enter directory
                    self.enter_directory();
                } else if is_video_file(&selected) {
                    // Select single file
                    self.files.clear();
                    self.files.push(VideoFile::new(selected));
                    self.analyze_files();
                }
            }
            SelectionMode::Folder => {
                if selected == Path::new("..") || !selected.is_dir() {
                    // Navigate up or ignore non-directories
                    self.enter_directory();
                } else {
                    // Select this folder and scan for videos
                    self.scan_folder_for_videos(&selected);
                    if self.files.is_empty() {
                        self.set_message("No video files found in this folder");
                    } else {
                        self.analyze_files();
                    }
                }
            }
        }
    }

    fn scan_folder_for_videos(&mut self, folder: &PathBuf) {
        self.files.clear();

        if let Ok(entries) = std::fs::read_dir(folder) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| is_video_file(p))
                .collect();

            paths.sort();

            for path in paths {
                self.files.push(VideoFile::new(path));
            }
        }
    }

    fn analyze_files(&mut self) {
        for file in &mut self.files {
            file.status = FileStatus::Analyzing;

            match analyze_full(file.path.to_str().unwrap_or("")) {
                Ok(analysis) => {
                    let resolution = analysis.video.classify_video().ok();
                    file.analysis = Some(analysis.video);
                    file.audio_tracks = analysis.audio_tracks;
                    file.subtitle_tracks = analysis.subtitle_tracks;
                    file.resolution = resolution;
                    file.select_all_tracks();
                    file.generate_output_path();

                    // Dolby Vision files are converted to HDR10
                    file.status = FileStatus::AwaitingConfig;
                }
                Err(e) => {
                    file.status = FileStatus::Error {
                        message: e.to_string(),
                    };
                    self.error_count += 1;
                }
            }
        }

        // Find first file awaiting config
        self.config_file_index = self
            .files
            .iter()
            .position(|f| matches!(f.status, FileStatus::AwaitingConfig))
            .unwrap_or(0);

        if self
            .files
            .iter()
            .any(|f| matches!(f.status, FileStatus::AwaitingConfig))
        {
            self.navigate_to_track_config();
        } else {
            // All files are either skipped or errored
            self.navigate_to_finish();
        }
    }

    pub fn current_config_file(&self) -> Option<&VideoFile> {
        self.files.get(self.config_file_index)
    }

    pub fn current_config_file_mut(&mut self) -> Option<&mut VideoFile> {
        self.files.get_mut(self.config_file_index)
    }

    pub fn confirm_track_config(&mut self) {
        if let Some(file) = self.files.get_mut(self.config_file_index) {
            file.status = FileStatus::ReadyToConvert;
        }

        // Find next file awaiting config
        let next_index = self
            .files
            .iter()
            .skip(self.config_file_index + 1)
            .position(|f| matches!(f.status, FileStatus::AwaitingConfig))
            .map(|i| i + self.config_file_index + 1);

        if let Some(idx) = next_index {
            self.config_file_index = idx;
            self.track_focus = TrackFocus::Audio;
            self.audio_cursor = 0;
            self.subtitle_cursor = 0;
        } else {
            // All files configured, start encoding
            self.start_encoding();
        }
    }

    pub fn start_encoding(&mut self) {
        info!("Starting encoding process");
        self.navigate_to_queue();
        self.encoding_active = true;
        self.current_file_index = 0;

        // Reset cancel flag
        self.cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag = self.cancel_flag.clone();

        let (tx, rx) = mpsc::channel();
        self.progress_receiver = Some(rx);

        // Get the encoder
        let encoder = self.encoder_config.selected_encoder;
        let run_vmaf = self.encoder_config.run_vmaf;
        let vmaf_threshold = self.encoder_config.vmaf_threshold;

        // Collect files to encode with their encode options
        let files_to_encode: Vec<(
            usize,
            PathBuf,
            PathBuf,
            Resolution,
            TrackSelection,
            EncodeOptions,
        )> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f.status, FileStatus::ReadyToConvert))
            .map(|(i, f)| {
                let track_selection = TrackSelection {
                    audio_tracks: f.selected_audio.clone(),
                    subtitle_tracks: f.selected_subtitles.clone(),
                };

                // Encode options based on file analysis
                let mut encode_options = if let Some(ref analysis) = f.analysis {
                    EncodeOptions::from_analysis(analysis, &f.filename())
                } else {
                    EncodeOptions {
                        content_type: ContentType::from_filename(&f.filename()),
                        ..Default::default()
                    }
                };

                // Apply VMAF settings from encoder config
                encode_options.run_vmaf = run_vmaf;
                encode_options.vmaf_threshold = vmaf_threshold;

                (
                    i,
                    f.path.clone(),
                    f.output_path.clone().unwrap_or_else(|| f.path.clone()),
                    f.resolution.unwrap_or(Resolution::HD1080p),
                    track_selection,
                    encode_options,
                )
            })
            .collect();

        info!("Files to encode: {}", files_to_encode.len());

        // Start timer and track total files
        self.start_time = Some(Instant::now());
        self.total_files_to_encode = files_to_encode.len();

        // Mark files as pending in queue
        for (idx, _, _, _, _, _) in &files_to_encode {
            if let Some(f) = self.files.get_mut(*idx) {
                f.status = FileStatus::Pending;
            }
        }

        // Start encoding thread
        thread::spawn(move || {
            debug!("Encoding thread started");
            for (idx, input, output, resolution, track_selection, encode_options) in files_to_encode
            {
                debug!("Processing file idx={}, input={:?}", idx, input);

                // Check if cancelled before starting next file
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = tx.send(ProgressMessage::Cancelled);
                    break;
                }

                let tx_clone = tx.clone();
                let cancel_clone = cancel_flag.clone();

                // Send initial progress
                let _ = tx.send(ProgressMessage::Progress(idx, 0.0));

                let result = encode_video(
                    input.to_str().unwrap_or(""),
                    output.to_str().unwrap_or(""),
                    resolution,
                    &track_selection,
                    encoder,
                    Some(Box::new(move |progress| {
                        let _ = tx_clone.send(ProgressMessage::Progress(idx, progress));
                    })),
                    cancel_clone,
                    &encode_options,
                );

                match result {
                    EncodeResult::Success => {
                        let _ = tx.send(ProgressMessage::Done(idx));
                    }
                    EncodeResult::SuccessWithVmaf(vmaf) => {
                        let _ = tx.send(ProgressMessage::DoneWithVmaf(idx, vmaf.score));
                    }
                    EncodeResult::Cancelled => {
                        let _ = tx.send(ProgressMessage::Cancelled);
                        break;
                    }
                    EncodeResult::Error(e) => {
                        let _ = tx.send(ProgressMessage::Error(idx, e));
                    }
                    EncodeResult::QualityBelowThreshold {
                        vmaf, threshold, ..
                    } => {
                        let _ =
                            tx.send(ProgressMessage::QualityWarning(idx, vmaf.score, threshold));
                    }
                }
            }
        });
    }

    pub fn cancel_encoding(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn process_progress_messages(&mut self) {
        // Collect messages first to avoid borrow conflicts
        let messages: Vec<ProgressMessage> = if let Some(ref rx) = self.progress_receiver {
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
                ProgressMessage::Progress(idx, progress) => {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.status = FileStatus::Converting { progress };
                        self.current_file_index = idx;
                    }
                }
                ProgressMessage::Done(idx) => {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.status = FileStatus::Done;
                        self.converted_count += 1;
                    }

                    // Check if all done
                    if self.all_files_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                ProgressMessage::DoneWithVmaf(idx, score) => {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.status = FileStatus::DoneWithVmaf { score };
                        self.converted_count += 1;
                    }

                    if self.all_files_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                ProgressMessage::Error(idx, msg) => {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.status = FileStatus::Error { message: msg };
                        self.error_count += 1;
                    }

                    if self.all_files_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                ProgressMessage::QualityWarning(idx, vmaf, threshold) => {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.status = FileStatus::QualityWarning { vmaf, threshold };
                        // Converted but with warning
                        self.converted_count += 1;
                    }

                    if self.all_files_completed() {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                ProgressMessage::Cancelled => {
                    // Mark current converting file as cancelled
                    for file in &mut self.files {
                        if matches!(file.status, FileStatus::Converting { .. }) {
                            file.status = FileStatus::Skipped {
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

    /// Check if all files have completed (success, error, skipped)
    fn all_files_completed(&self) -> bool {
        self.files.iter().all(|f| {
            matches!(
                f.status,
                FileStatus::Done
                    | FileStatus::DoneWithVmaf { .. }
                    | FileStatus::Skipped { .. }
                    | FileStatus::Error { .. }
                    | FileStatus::QualityWarning { .. }
            )
        })
    }

    pub fn reset(&mut self) {
        self.files.clear();
        self.current_file_index = 0;
        self.config_file_index = 0;
        self.converted_count = 0;
        self.skipped_count = 0;
        self.error_count = 0;
        self.encoding_active = false;
        self.progress_receiver = None;
        self.start_time = None;
        self.total_files_to_encode = 0;
        self.navigate_to_home();
    }

    /// Get the elapsed time since the queue started encoding
    pub fn queue_elapsed_time(&self) -> Option<std::time::Duration> {
        self.start_time.map(|start| start.elapsed())
    }

    /// Calculate the overall queue progress (0.0 to 100.0)
    /// Takes into account completed files and current file progress
    pub fn queue_overall_progress(&self) -> f32 {
        if self.total_files_to_encode == 0 {
            return 0.0;
        }

        // Count completed files in the queue (Done status, not skipped before encoding)
        let completed = self
            .files
            .iter()
            .filter(|f| matches!(f.status, FileStatus::Done | FileStatus::DoneWithVmaf { .. }))
            .count();

        // Get current file progress
        let current_progress = self
            .files
            .get(self.current_file_index)
            .and_then(|f| {
                if let FileStatus::Converting { progress } = f.status {
                    Some(progress)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);

        // Overall progress: (completed files * 100 + current file progress) / total files
        let total_progress =
            (completed as f32 * 100.0 + current_progress) / self.total_files_to_encode as f32;
        total_progress.min(100.0)
    }

    /// Get the estimated time remaining for the entire queue
    pub fn queue_estimated_time_remaining(&self) -> Option<std::time::Duration> {
        let progress = self.queue_overall_progress();
        if progress <= 0.0 || progress >= 100.0 {
            return None;
        }
        let elapsed = self.queue_elapsed_time()?;
        let elapsed_secs = elapsed.as_secs_f64();
        let total_estimated_secs = elapsed_secs / (progress as f64 / 100.0);
        let remaining_secs = total_estimated_secs - elapsed_secs;
        if remaining_secs > 0.0 {
            Some(std::time::Duration::from_secs_f64(remaining_secs))
        } else {
            None
        }
    }
}

/// Format a duration as HH:MM:SS or MM:SS
pub fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}
