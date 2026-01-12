use crate::analysis::{analyze_full, Resolution};
use crate::converter::{encode_video, EncodeResult, TrackSelection};
use crate::data::{is_video_file, FileStatus, VideoFile};
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

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

    // Stats
    pub converted_count: usize,
    pub skipped_count: usize,
    pub error_count: usize,

    // Message/notification
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackFocus {
    Audio,
    Subtitle,
    Confirm,
}

pub enum ProgressMessage {
    Progress(usize, f32),
    Done(usize),
    Error(usize, String),
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
            converted_count: 0,
            skipped_count: 0,
            error_count: 0,
            message: None,
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
        if let Some(parent) = self.current_dir.parent() {
            if parent != self.current_dir {
                self.dir_entries.push(PathBuf::from(".."));
            }
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
            paths.sort_by(|a, b| {
                match (a.is_dir(), b.is_dir()) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.file_name().cmp(&b.file_name()),
                }
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

        if selected == PathBuf::from("..") {
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
                if selected == PathBuf::from("..") {
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
                if selected == PathBuf::from("..") || !selected.is_dir() {
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

                    // Check for Dolby Vision
                    if file.is_dolby_vision() {
                        file.status = FileStatus::Skipped {
                            reason: "Dolby Vision".to_string(),
                        };
                        self.skipped_count += 1;
                    } else {
                        file.status = FileStatus::AwaitingConfig;
                    }
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
        self.config_file_index = self.files
            .iter()
            .position(|f| matches!(f.status, FileStatus::AwaitingConfig))
            .unwrap_or(0);

        if self.files.iter().any(|f| matches!(f.status, FileStatus::AwaitingConfig)) {
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
        let next_index = self.files
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
        self.navigate_to_queue();
        self.encoding_active = true;
        self.current_file_index = 0;

        // Reset cancel flag
        self.cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag = self.cancel_flag.clone();

        let (tx, rx) = mpsc::channel();
        self.progress_receiver = Some(rx);

        // Collect files to encode
        let files_to_encode: Vec<(usize, PathBuf, PathBuf, Resolution, TrackSelection)> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f.status, FileStatus::ReadyToConvert))
            .map(|(i, f)| {
                let track_selection = TrackSelection {
                    audio_tracks: f.selected_audio.clone(),
                    subtitle_tracks: f.selected_subtitles.clone(),
                };
                (
                    i,
                    f.path.clone(),
                    f.output_path.clone().unwrap_or_else(|| f.path.clone()),
                    f.resolution.unwrap_or(Resolution::HD1080p),
                    track_selection,
                )
            })
            .collect();

        // Mark files as pending in queue
        for (idx, _, _, _, _) in &files_to_encode {
            if let Some(f) = self.files.get_mut(*idx) {
                f.status = FileStatus::Pending;
            }
        }

        // Start encoding thread
        thread::spawn(move || {
            for (idx, input, output, resolution, track_selection) in files_to_encode {
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
                    Some(Box::new(move |progress| {
                        let _ = tx_clone.send(ProgressMessage::Progress(idx, progress));
                    })),
                    cancel_clone,
                );

                match result {
                    EncodeResult::Success => {
                        let _ = tx.send(ProgressMessage::Done(idx));
                    }
                    EncodeResult::Cancelled => {
                        let _ = tx.send(ProgressMessage::Cancelled);
                        break;
                    }
                    EncodeResult::Error(e) => {
                        let _ = tx.send(ProgressMessage::Error(idx, e));
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
                    if self.files.iter().all(|f| {
                        matches!(
                            f.status,
                            FileStatus::Done
                                | FileStatus::Skipped { .. }
                                | FileStatus::Error { .. }
                        )
                    }) {
                        self.encoding_active = false;
                        should_finish = true;
                    }
                }
                ProgressMessage::Error(idx, msg) => {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.status = FileStatus::Error { message: msg };
                        self.error_count += 1;
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

    pub fn reset(&mut self) {
        self.files.clear();
        self.current_file_index = 0;
        self.config_file_index = 0;
        self.converted_count = 0;
        self.skipped_count = 0;
        self.error_count = 0;
        self.encoding_active = false;
        self.progress_receiver = None;
        self.navigate_to_home();
    }
}
