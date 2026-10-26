use crate::analyzer::{self, AnalysisResult, DvMode, HdrType, is_av1_codec};
use crate::config::{AppConfig, Encoder};
use crate::daemon::client::{ClientError, ClientEvent, ClientRequest, RequestKind};
use crate::disc::worker::DiscEvent;
use crate::disc::{DiscDrive, DiscError, DiscSource, DiscTitle};
use crate::error::AppError;
use crate::queue::{
    EncodingJob, JobStatus, QueueState, WorkerJob, WorkerMessage, auto_select_tracks,
    is_video_file, make_output_paths_unique, run_worker,
};
use crate::utils::DependencyStatus;
use ratatui::widgets::ListState;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Instant;
use tracing::{info, warn};

/// Application screens
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Home,
    FileExplorer,
    FileConfirm,
    DiscDrives,
    DiscTitles,
    TrackConfig,
    Queue,
    Finish,
    Configuration,
}

/// Current disc discovery, scan, or rip UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscState {
    Discovering,
    Scanning,
    Cancelling,
    Ready,
    /// Already translated, ready to render.
    Failed(String),
}

/// Cached metadata for one file-explorer row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
    /// `None` for directories and for files whose metadata could not be read.
    pub size: Option<u64>,
}

impl Entry {
    /// The ".." row, which carries no metadata of its own.
    fn parent() -> Self {
        Self {
            path: PathBuf::from(".."),
            is_dir: true,
            size: None,
        }
    }

    pub fn is_parent(&self) -> bool {
        self.path == Path::new("..")
    }

    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.to_string_lossy().to_string(),
            |n| n.to_string_lossy().to_string(),
        )
    }
}

/// File selection mode
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionMode {
    File,
    Folder,
    FolderRecursive,
    /// Picking a ripped disc folder or an ISO image instead of a video file.
    DiscFolder,
    /// Picking the folder for the path setting selected on the Settings screen.
    SettingFolder,
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
    CancelDisc,
    ExitApp,
    AbandonTrackConfig,
    DiscardConfigChanges,
    CancelAnalysis,
    NewConversion,
    /// Remove the ripped title at this queue index, deleting its staging files.
    RemoveRip(usize),
    /// Clear finished jobs, some of them ripped titles.
    ClearFinishedRips,
}

impl ConfirmAction {
    /// Whether the dialog acts on jobs of the daemon run it was opened under.
    fn names_queue_jobs(self) -> bool {
        matches!(
            self,
            Self::RemoveRip(_)
                | Self::ClearFinishedRips
                | Self::CancelEncoding
                | Self::CancelAnalysis
                | Self::CancelDisc
                | Self::AbandonTrackConfig
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Success,
    Warning,
    Error,
}

/// Where the queue runs and encodes.
pub enum Backend {
    /// In this process.
    Local,
    /// In the running daemon.
    Remote(RemoteSession),
}

/// Connection to the daemon this TUI is attached to.
// Independent connection flags, read one at a time.
#[allow(clippy::struct_excessive_bools)]
pub struct RemoteSession {
    pub requests: mpsc::Sender<ClientRequest>,
    pub events: Receiver<ClientEvent>,
    /// Daemon job ids, `ids[i]` for `queue.jobs[i]`.
    pub ids: Vec<u64>,
    /// `taken_ms` of the last applied snapshot.
    pub last_taken_ms: u64,
    /// Daemon run id of the last snapshot.
    pub instance: u64,
    pub reachable: bool,
    /// The daemon answered 401 to this TUI's token.
    pub token_rejected: bool,
    /// Version of a daemon run that differs from this TUI's version.
    pub version_mismatch: Option<String>,
    /// The daemon's host-level settings are read-only for this TUI.
    pub host_settings_read_only: bool,
    /// Jobs shown on the file confirmation screen and not yet sent.
    pub pending: Vec<EncodingJob>,
    /// Folder and add mode the pending jobs were previewed from.
    pub pending_folder: Option<(String, &'static str)>,
    /// Folder previews cancelled before their reply arrived.
    pub cancelled_previews: usize,
    pub was_encoding: bool,
    /// The last snapshot had jobs pending or analyzing.
    pub was_analyzing: bool,
    /// The daemon runs a drive listing, disc scan or rip.
    pub disc_active: bool,
    /// Job id and `apply_to_remaining` of the track selection awaiting its reply.
    pub tracks_in_flight: Option<(u64, bool)>,
}

impl RemoteSession {
    /// A reachable session with no snapshot applied yet.
    pub fn new(requests: mpsc::Sender<ClientRequest>, events: Receiver<ClientEvent>) -> Self {
        Self {
            requests,
            events,
            ids: Vec::new(),
            last_taken_ms: 0,
            instance: 0,
            reachable: true,
            token_rejected: false,
            version_mismatch: None,
            host_settings_read_only: false,
            pending: Vec::new(),
            pending_folder: None,
            cancelled_previews: 0,
            was_encoding: false,
            was_analyzing: false,
            disc_active: false,
            tracks_in_flight: None,
        }
    }

    /// Takes the daemon run of `snapshot` as current when it differs from the
    /// last one: resets the snapshot clock, encode tracking and the track
    /// selection in flight, and records a version mismatch. Returns whether
    /// the run changed.
    fn follow_instance(&mut self, snapshot: &crate::daemon::client::RemoteSnapshot) -> bool {
        if snapshot.instance == self.instance {
            return false;
        }
        self.instance = snapshot.instance;
        self.last_taken_ms = 0;
        self.tracks_in_flight = None;
        self.was_encoding = snapshot.encoding_active;
        self.version_mismatch =
            (snapshot.version != env!("CARGO_PKG_VERSION")).then(|| snapshot.version.clone());
        true
    }
}

/// Main application state
// Flat screen-state flags, read one at a time.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub current_screen: Screen,
    pub should_quit: bool,
    pub selection_mode: SelectionMode,

    // File explorer
    pub current_dir: PathBuf,
    pub dir_entries: Vec<Entry>,
    pub explorer_index: usize,
    pub explorer_list_state: ListState,
    // Queue state
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
    /// Index of the first job added by the current explorer selection.
    pub batch_start: usize,
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
    pub disc_drive_receiver: Option<Receiver<Result<Vec<DiscDrive>, DiscError>>>,
    pub disc_receiver: Option<Receiver<DiscEvent>>,
    pub disc_cancel_flag: Arc<AtomicBool>,
    /// Queue indices of titles in the current rip, in request order.
    disc_job_indices: Vec<usize>,
    /// What the current scan or rip is reading from.
    pub disc_source: Option<DiscSource>,

    // Encoding
    pub encoding_active: bool,
    /// Lock file held while this TUI encodes locally.
    pub tui_lock: Option<crate::daemon::lifecycle::TuiLockGuard>,
    /// Process id of a running daemon, checked before a local encode starts.
    pub daemon_running: fn() -> Option<u32>,
    pub progress_receiver: Option<Receiver<WorkerMessage>>,
    pub cancel_flag: Arc<AtomicBool>,
    /// Queue indices assigned to the current encoding worker session.
    encoding_session_indices: Vec<usize>,

    // Background analysis. Results are keyed by job index: a title that has
    // just finished ripping joins the same channel while others still probe.
    pub analysis_receiver: Option<Receiver<(usize, Result<AnalysisResult, AppError>)>>,
    analysis_sender: Option<mpsc::Sender<(usize, Result<AnalysisResult, AppError>)>>,
    /// Probes handed out and not yet applied.
    analysis_outstanding: usize,
    /// Ask the analysis thread to stop spawning new ffprobe calls
    pub analysis_cancel_flag: Arc<AtomicBool>,

    // Folder scan state
    pub folder_scan_receiver: Option<Receiver<Result<Vec<PathBuf>, String>>>,
    pub folder_scan_cancel_flag: Arc<AtomicBool>,

    // Configuration
    pub config: AppConfig,
    /// ffmpeg and ffprobe are on PATH — without these nothing works
    pub deps: bool,
    /// This `FFmpeg` build has libvmaf, which only VMAF verification needs
    pub vmaf_deps: bool,
    /// Whether this `FFmpeg` build can encode Opus
    pub opus_deps: bool,
    /// Whether the configured encoder exists in this `FFmpeg` build
    pub encoder_deps: bool,

    // UI state
    pub message: Option<String>,
    pub message_kind: MessageKind,
    pub message_expiry: Option<Instant>,
    pub confirm_dialog: Option<(ConfirmAction, bool)>,
    /// Dolby Vision mode dialog: selected option (0 = keep DV, 1 = HDR10)
    pub dv_dialog: Option<usize>,
    /// Vertical offset for the active screen's detail panel.
    pub detail_scroll: u16,

    // Config screen state
    pub config_selected: usize,
    pub config_edit_buffer: Option<String>,
    pub config_edit_cursor: usize,
    /// The configuration as last loaded or saved. Encodes, analysis and track
    /// choices read it; the Settings screen edits `config`.
    pub saved_config: AppConfig,

    // Finish screen
    pub finish_cursor: usize,
    pub finish_list_state: ListState,

    pub backend: Backend,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let (config, load_error) = AppConfig::load_reporting();
        Self::with_config(config, load_error)
    }

    /// An app attached to the daemon, showing its first snapshot; sends `SettingsAccess`.
    pub fn attached(
        config: AppConfig,
        session: RemoteSession,
        first: &crate::daemon::client::RemoteSnapshot,
    ) -> Self {
        let mut app = Self::with_config(config, None);
        let (queue, ids) = first.queue_state();
        app.queue = queue;
        app.encoding_active = first.encoding_active;
        if first.encoding_active {
            app.current_screen = Screen::Queue;
        }
        let _ = session.requests.send(ClientRequest::SettingsAccess);
        app.backend = Backend::Remote(RemoteSession {
            ids,
            last_taken_ms: first.taken_ms,
            instance: first.instance,
            reachable: true,
            was_encoding: first.encoding_active,
            disc_active: first.disc.active,
            ..session
        });
        let analyzing = app.analysis_active();
        if let Some(remote) = app.remote_mut() {
            remote.was_analyzing = analyzing;
        }
        app
    }

    fn remote_mut(&mut self) -> Option<&mut RemoteSession> {
        match &mut self.backend {
            Backend::Remote(remote) => Some(remote),
            Backend::Local => None,
        }
    }

    /// Sends `request` to the attached daemon; returns whether this app is attached.
    pub fn send_to_daemon(&self, request: ClientRequest) -> bool {
        let Backend::Remote(remote) = &self.backend else {
            return false;
        };
        let _ = remote.requests.send(request);
        true
    }

    /// The daemon id of the job at `index`.
    fn daemon_id(&self, index: usize) -> Option<u64> {
        match &self.backend {
            Backend::Remote(remote) => remote.ids.get(index).copied(),
            Backend::Local => None,
        }
    }

    /// Whether the attached daemon treats this TUI as a remote client.
    pub fn host_settings_read_only(&self) -> bool {
        matches!(&self.backend, Backend::Remote(remote) if remote.host_settings_read_only)
    }

    pub fn is_attached(&self) -> bool {
        matches!(self.backend, Backend::Remote(_))
    }

    /// Asks to quit: an attached app with work running in the daemon and no
    /// unsaved settings quits at once, any other app opens the exit
    /// confirmation.
    pub fn request_exit(&mut self) {
        let unsaved = self.current_screen == Screen::Configuration && self.config_is_dirty();
        if self.is_attached() && self.work_active() && !unsaved {
            self.should_quit = true;
        } else {
            self.confirm_dialog = Some((ConfirmAction::ExitApp, false));
        }
    }

    /// Whether the attached daemon is unreachable, rejects this TUI's token,
    /// or runs a different version.
    pub fn daemon_blocked(&self) -> bool {
        self.blocked_status().is_some()
    }

    /// The status line shown while the attached daemon is blocked.
    fn blocked_status(&self) -> Option<String> {
        use crate::i18n::{Msg, t};
        let Backend::Remote(remote) = &self.backend else {
            return None;
        };
        let lang = self.config.language;
        if let Some(version) = &remote.version_mismatch {
            Some(
                t(lang, Msg::DaemonVersionMismatch)
                    .replace("{daemon}", version)
                    .replace("{tui}", env!("CARGO_PKG_VERSION")),
            )
        } else if remote.token_rejected {
            Some(t(lang, Msg::DaemonTokenRejected).to_string())
        } else if !remote.reachable {
            Some(t(lang, Msg::DaemonUnreachableRetrying).to_string())
        } else {
            None
        }
    }

    /// Applies every pending event from the attached daemon without blocking.
    pub fn process_client_events(&mut self) {
        let Backend::Remote(remote) = &self.backend else {
            return;
        };
        let events: Vec<ClientEvent> = remote.events.try_iter().collect();
        let before = self.blocked_status();
        for event in events {
            match event {
                ClientEvent::Snapshot(snapshot) => self.apply_snapshot(&snapshot),
                ClientEvent::Unreachable(reason) => {
                    warn!("Daemon unreachable: {reason}");
                    self.set_reachable(false);
                    self.set_token_rejected(false);
                }
                ClientEvent::TokenRejected => {
                    warn!("Daemon rejected the access token");
                    self.set_reachable(true);
                    self.set_token_rejected(true);
                }
                ClientEvent::Reachable => {
                    self.set_reachable(true);
                    self.set_token_rejected(false);
                    self.send_to_daemon(ClientRequest::SettingsAccess);
                }
                ClientEvent::Reply { request, result } => self.apply_reply(request, result),
            }
        }
        let after = self.blocked_status();
        if after != before {
            if before.is_some() && self.message == before {
                self.clear_message();
            }
            if let Some(status) = &after {
                self.set_message(status);
            }
        } else if let Some(status) = &after
            && self.message.is_none()
        {
            self.set_message(status);
        }
    }

    /// Applies the daemon's answer to a request this app sent.
    fn apply_reply(&mut self, request: RequestKind, result: Result<Value, ClientError>) {
        if request == RequestKind::AddPreview
            && let Some(remote) = self.remote_mut()
            && remote.cancelled_previews > 0
        {
            remote.cancelled_previews -= 1;
            return;
        }
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                match &error {
                    ClientError::Http { status: 401, .. } => self.set_token_rejected(true),
                    ClientError::Http { error, .. } => self.set_timed_error(error, 5),
                    ClientError::Unreachable(_) | ClientError::BadResponse(_) => {
                        let message = crate::i18n::t(
                            self.config.language,
                            crate::i18n::Msg::DaemonRequestFailed,
                        )
                        .replace("{error}", &error.to_string());
                        self.set_timed_error(&message, 5);
                    }
                }
                self.request_failed(request);
                return;
            }
        };
        let lang = self.config.language;
        match request {
            RequestKind::Browse => self.show_daemon_directory(&value),
            RequestKind::AddPreview => self.accept_previewed_files(&value),
            RequestKind::ClearFinished => {
                let removed = value["removed"].as_u64().unwrap_or(0);
                let message = crate::i18n::t(lang, crate::i18n::Msg::WebRemovedFinished)
                    .replace("{n}", &removed.to_string());
                self.set_timed_success(&message, 3);
            }
            RequestKind::SetTracks => self.tracks_applied(&value),
            RequestKind::DiscsList => {
                let Ok(drives) = serde_json::from_value::<Vec<DiscDrive>>(value["drives"].clone())
                else {
                    return;
                };
                self.disc_drives = drives;
                self.disc_state = DiscState::Ready;
                if self.current_screen == Screen::DiscDrives && self.disc_drives.len() == 1 {
                    self.scan_disc(0);
                }
            }
            RequestKind::SettingsGet => self.load_daemon_settings(value, false),
            RequestKind::SettingsSet => self.load_daemon_settings(value, true),
            RequestKind::SettingsAccess => {
                let local = value["local"].as_bool().unwrap_or(false);
                if let Some(remote) = self.remote_mut() {
                    remote.host_settings_read_only = !local;
                }
            }
            RequestKind::ServiceSet => {
                let installed = value["enabled"].as_bool().unwrap_or(false);
                if let Some(enabled) = value["daemon_enabled"].as_bool() {
                    self.config.daemon.enabled = enabled;
                    self.saved_config.daemon.enabled = enabled;
                }
                let msg = if installed {
                    crate::i18n::Msg::DaemonServiceInstalled
                } else {
                    crate::i18n::Msg::DaemonServiceUninstalled
                };
                self.set_timed_success(crate::i18n::t(lang, msg), 5);
            }
            RequestKind::Add | RequestKind::AddFolder => {
                if value["added"].as_u64() == Some(0) {
                    self.set_timed_message(
                        crate::i18n::t(lang, crate::i18n::Msg::WebNothingAdded),
                        4,
                    );
                }
            }
            RequestKind::Remove
            | RequestKind::MoveUp
            | RequestKind::Cancel
            | RequestKind::CancelAnalysis
            | RequestKind::DiscsScan
            | RequestKind::DiscsRip
            | RequestKind::DiscsCancel => {}
        }
    }

    /// Settles the screen after the daemon refused `request`.
    fn request_failed(&mut self, request: RequestKind) {
        match request {
            // A refused folder falls back once to the daemon's start folder.
            RequestKind::Browse if !self.current_dir.as_os_str().is_empty() => {
                self.current_dir = PathBuf::new();
                self.refresh_dir_entries();
            }
            RequestKind::AddPreview => {
                if let Some(remote) = self.remote_mut() {
                    remote.pending_folder = None;
                }
            }
            RequestKind::SetTracks => {
                if let Some(remote) = self.remote_mut() {
                    remote.tracks_in_flight = None;
                }
                if self.current_screen == Screen::TrackConfig {
                    self.navigate_to_queue();
                }
            }
            _ => {}
        }
    }

    /// Fills the explorer from a daemon directory listing.
    fn show_daemon_directory(&mut self, listing: &Value) {
        let Some(path) = listing["path"].as_str() else {
            return;
        };
        self.current_dir = PathBuf::from(path);
        self.dir_entries.clear();
        if listing["parent"].is_string() {
            self.dir_entries.push(Entry::parent());
        }
        let names = |key: &str| listing[key].as_array().cloned().unwrap_or_default();
        for dir in names("dirs") {
            if let Some(name) = dir["name"].as_str() {
                self.dir_entries.push(Entry {
                    path: self.current_dir.join(name),
                    is_dir: true,
                    size: None,
                });
            }
        }
        let disc_images = self.selection_mode == SelectionMode::DiscFolder;
        for file in names("files") {
            let listed = file["is_video"].as_bool() == Some(true)
                || (disc_images && file["is_iso"].as_bool() == Some(true));
            if let (true, Some(name)) = (listed, file["name"].as_str()) {
                self.dir_entries.push(Entry {
                    path: self.current_dir.join(name),
                    is_dir: false,
                    size: file["size"].as_u64(),
                });
            }
        }
        self.explorer_index = 0;
        self.explorer_list_state.select(Some(0));
    }

    /// Shows the files a folder preview found, when the explorer still waits for them.
    fn accept_previewed_files(&mut self, preview: &Value) {
        let waiting =
            matches!(&self.backend, Backend::Remote(remote) if remote.pending_folder.is_some());
        if !waiting || self.current_screen != Screen::FileExplorer {
            if let Some(remote) = self.remote_mut() {
                remote.pending_folder = None;
            }
            return;
        }
        let paths = preview["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .collect();
        self.accept_scanned_files(paths);
    }

    /// Marks the confirmed job Ready and, when it is still the one on Track
    /// Config, moves on.
    fn tracks_applied(&mut self, applied: &Value) {
        let Some(remote) = self.remote_mut() else {
            return;
        };
        let Some((id, apply_to_remaining)) = remote.tracks_in_flight.take() else {
            return;
        };
        let index = remote.ids.iter().position(|&other| other == id);
        if let Some(job) = index.and_then(|index| self.queue.jobs.get_mut(index))
            && matches!(job.status, JobStatus::AwaitingConfig)
        {
            job.status = JobStatus::Ready;
        }
        if self.current_screen != Screen::TrackConfig {
            return;
        }
        if apply_to_remaining {
            let count = applied["applied"].as_u64().unwrap_or(1);
            let message = crate::i18n::t(self.config.language, crate::i18n::Msg::WebTracksApplied)
                .replace("{n}", &count.to_string());
            self.detail_scroll = 0;
            self.navigate_to_queue();
            self.set_timed_success(&message, 4);
            return;
        }
        if index == Some(self.queue.config_job_index) {
            self.advance_track_config();
        }
    }

    /// Takes the daemon's configuration as both the live and the saved one;
    /// a loaded copy leaves unsaved edits alone.
    fn load_daemon_settings(&mut self, settings: Value, saved: bool) {
        let warning = settings["_warning"].as_str().map(str::to_string);
        let Ok(config) = serde_json::from_value::<AppConfig>(settings) else {
            return;
        };
        if !saved && self.config_is_dirty() {
            return;
        }
        self.config = config.clone();
        self.saved_config = config;
        if !saved {
            return;
        }
        if let Some(warning) = warning {
            self.set_timed_error(&warning, 6);
        } else {
            let msg = crate::i18n::t(self.config.language, crate::i18n::Msg::SavedExclaim);
            self.set_timed_success(msg, 3);
        }
    }

    fn set_reachable(&mut self, reachable: bool) {
        if let Backend::Remote(remote) = &mut self.backend {
            remote.reachable = reachable;
        }
    }

    fn set_token_rejected(&mut self, rejected: bool) {
        if let Backend::Remote(remote) = &mut self.backend {
            remote.token_rejected = rejected;
        }
    }

    /// Mirrors a daemon snapshot newer than the last one applied, keeping the
    /// job open on Track Config and the jobs pending on File Confirm.
    fn apply_snapshot(&mut self, snapshot: &crate::daemon::client::RemoteSnapshot) {
        let Backend::Remote(remote) = &mut self.backend else {
            return;
        };
        let restarted = remote.follow_instance(snapshot);
        let stale = remote.version_mismatch.is_some() || snapshot.taken_ms < remote.last_taken_ms;
        if restarted {
            self.leave_previous_run();
        }
        let Backend::Remote(remote) = &mut self.backend else {
            return;
        };
        if stale {
            return;
        }
        remote.token_rejected = false;
        let editing = (self.current_screen == Screen::TrackConfig)
            .then(|| {
                let index = self.queue.config_job_index;
                Some((*remote.ids.get(index)?, self.queue.jobs.get(index)?.clone()))
            })
            .flatten();
        let cursor_id = remote.ids.get(self.queue_cursor).copied();
        let rip_removal = match self.confirm_dialog {
            Some((ConfirmAction::RemoveRip(index), selected)) => {
                Some((remote.ids.get(index).copied(), selected))
            }
            _ => None,
        };
        let (queue, ids) = snapshot.queue_state();
        self.queue = queue;
        remote.ids = ids;
        remote.last_taken_ms = snapshot.taken_ms;
        remote.disc_active = snapshot.disc.active;
        let was_encoding = std::mem::replace(&mut remote.was_encoding, snapshot.encoding_active);
        let position = |id: Option<u64>| id.and_then(|id| remote.ids.iter().position(|&o| o == id));
        if let Some(index) = position(cursor_id) {
            self.queue_cursor = index;
        }
        if let Some((id, selected)) = rip_removal {
            self.confirm_dialog =
                position(id).map(|index| (ConfirmAction::RemoveRip(index), selected));
        }
        if self.current_screen == Screen::FileConfirm {
            self.batch_start = self.queue.jobs.len();
            self.queue.jobs.extend(remote.pending.iter().cloned());
        }
        let edited_index = editing.map(|(id, job)| {
            let index = remote.ids.iter().position(|&other| other == id);
            if let Some(index) = index {
                self.queue.jobs[index] = job;
                self.queue.config_job_index = index;
            }
            index
        });

        self.mirror_disc(&snapshot.disc);
        self.encoding_active = snapshot.encoding_active;

        if edited_index == Some(None) {
            let lang = self.config.language;
            self.set_timed_error(
                crate::i18n::t(lang, crate::i18n::Msg::JobRemovedByDaemon),
                5,
            );
            self.navigate_to_queue();
        }
        if was_encoding && !snapshot.encoding_active && self.follows_background_work() {
            self.navigate_to_finish();
        }
        let analyzing = self.analysis_active();
        let was_analyzing = self
            .remote_mut()
            .is_some_and(|remote| std::mem::replace(&mut remote.was_analyzing, analyzing));
        if was_analyzing
            && !restarted
            && !analyzing
            && !self.encoding_active
            && self.follows_background_work()
            && self.current_screen != Screen::TrackConfig
            && self.has_jobs_awaiting_config()
        {
            self.configure_next_job();
        }
        let last_job = self.queue.jobs.len().saturating_sub(1);
        self.queue_cursor = self.queue_cursor.min(last_job);
        self.finish_cursor = self.finish_cursor.min(last_job);
        self.disc_cursor = self
            .disc_cursor
            .min(self.disc_titles.len().saturating_sub(1));
        self.disc_drive_cursor = self.disc_drive_cursor.min(self.disc_drives.len());
    }

    /// Closes the confirm dialogs about queue jobs and leaves Track
    /// configuration for the Queue, discarding its edit, with a message.
    fn leave_previous_run(&mut self) {
        if self
            .confirm_dialog
            .is_some_and(|(action, _)| action.names_queue_jobs())
        {
            self.confirm_dialog = None;
        }
        if self.current_screen == Screen::TrackConfig {
            let lang = self.config.language;
            self.set_timed_error(
                crate::i18n::t(lang, crate::i18n::Msg::DaemonRestartedEditDiscarded),
                5,
            );
            self.navigate_to_queue();
        }
    }

    /// Mirrors the daemon's disc drives, titles, source and run state.
    fn mirror_disc(&mut self, disc: &crate::daemon::client::RemoteDisc) {
        self.disc_drives.clone_from(&disc.drives);
        self.disc_titles.clone_from(&disc.titles);
        self.disc_source.clone_from(&disc.scanned_source);
        self.disc_state = if let Some(error) = &disc.error {
            DiscState::Failed(error.clone())
        } else if disc.scanning {
            DiscState::Scanning
        } else if disc.active {
            DiscState::Ready
        } else if disc.listing {
            DiscState::Discovering
        } else {
            DiscState::Ready
        };
    }

    #[allow(clippy::too_many_lines)]
    fn with_config(config: AppConfig, load_error: Option<String>) -> Self {
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

        let deps = DependencyStatus::check();
        let vmaf_deps = DependencyStatus::vmaf_available();
        let opus_deps = DependencyStatus::libopus_available();
        let encoder_name = config.encoder.ffmpeg_name();
        let encoder_deps = DependencyStatus::encoder_available(encoder_name);

        info!("Using encoder: {}", config.encoder);

        let encoder_warning_expires = load_error.is_none() && !encoder_deps;
        let (message, message_kind) = if let Some(error) = load_error {
            (
                Some(format!(
                    "{} ({})",
                    crate::i18n::t(config.language, crate::i18n::Msg::ConfigLoadFailed),
                    error.lines().next().unwrap_or_default()
                )),
                MessageKind::Warning,
            )
        } else if !encoder_deps {
            (
                Some(
                    crate::i18n::t(config.language, crate::i18n::Msg::EncoderUnavailable)
                        .to_string(),
                ),
                MessageKind::Warning,
            )
        } else {
            (None, MessageKind::Info)
        };

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
            batch_start: 0,
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
            disc_drive_receiver: None,
            disc_receiver: None,
            disc_cancel_flag: Arc::new(AtomicBool::new(false)),
            disc_job_indices: Vec::new(),
            disc_source: None,
            encoding_active: false,
            tui_lock: None,
            daemon_running: if cfg!(test) {
                || None
            } else {
                crate::daemon::lifecycle::running_pid
            },
            progress_receiver: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            encoding_session_indices: Vec::new(),
            analysis_receiver: None,
            analysis_sender: None,
            analysis_outstanding: 0,
            analysis_cancel_flag: Arc::new(AtomicBool::new(false)),
            folder_scan_receiver: None,
            folder_scan_cancel_flag: Arc::new(AtomicBool::new(false)),
            deps,
            vmaf_deps,
            opus_deps,
            encoder_deps,
            message,
            message_kind,
            message_expiry: encoder_warning_expires
                .then(|| Instant::now() + std::time::Duration::from_secs(8)),
            confirm_dialog: None,
            dv_dialog: None,
            detail_scroll: 0,
            config_selected: 0,
            config_edit_buffer: None,
            config_edit_cursor: 0,
            saved_config: config.clone(),
            config,
            finish_cursor: 0,
            finish_list_state,
            backend: Backend::Local,
        }
    }

    // Message handling

    pub fn set_message(&mut self, msg: &str) {
        self.set_message_kind(msg, MessageKind::Warning, None);
    }

    pub fn set_info_message(&mut self, msg: &str) {
        self.set_message_kind(msg, MessageKind::Info, None);
    }

    pub fn set_timed_success(&mut self, msg: &str, secs: u64) {
        self.set_message_kind(msg, MessageKind::Success, Some(secs));
    }

    pub fn set_timed_error(&mut self, msg: &str, secs: u64) {
        self.set_message_kind(msg, MessageKind::Error, Some(secs));
    }

    fn set_message_kind(&mut self, msg: &str, kind: MessageKind, secs: Option<u64>) {
        self.message = Some(msg.to_string());
        self.message_kind = kind;
        self.message_expiry =
            secs.map(|secs| Instant::now() + std::time::Duration::from_secs(secs));
    }

    pub fn set_timed_message(&mut self, msg: &str, secs: u64) {
        self.set_message_kind(msg, MessageKind::Warning, Some(secs));
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
        self.current_screen = Screen::FileExplorer;
    }

    /// Open the explorer in folder mode for the selected path setting,
    /// starting at `start` when it is a directory.
    pub fn browse_for_setting(&mut self, start: Option<&str>) {
        if let Some(dir) = start.map(PathBuf::from).filter(|dir| dir.is_dir()) {
            self.current_dir = dir;
        }
        self.selection_mode = SelectionMode::SettingFolder;
        self.refresh_dir_entries();
        self.current_screen = Screen::FileExplorer;
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

        if self.saved_config.encoder == Encoder::SvtAv1 {
            let recommended = DvMode::recommended_for(meta.dv_profile);
            self.dv_dialog = Some(dv_mode_index(recommended));
        } else if let Some(job) = self.current_config_job_mut() {
            job.dv_mode = Some(DvMode::ToHdr10);
        }
    }

    /// Re-open the DV-mode dialog from the track config screen ('d' key).
    pub fn reopen_dv_dialog(&mut self) {
        let Some(job) = self.current_config_job() else {
            return;
        };
        if job.remux_only {
            return;
        }
        let Some(meta) = job.metadata.as_ref() else {
            return;
        };
        if meta.hdr_type != HdrType::DolbyVision {
            return;
        }
        if self.saved_config.encoder != Encoder::SvtAv1 {
            let msg = crate::i18n::t(self.config.language, crate::i18n::Msg::DvRequiresSvt);
            self.set_timed_message(msg, 3);
            return;
        }
        let current = job
            .dv_mode
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

    /// Whether finished background work may move the user on: true on the
    /// Queue, Track Config and Finish screens.
    fn follows_background_work(&self) -> bool {
        matches!(
            self.current_screen,
            Screen::Queue | Screen::TrackConfig | Screen::Finish
        )
    }

    pub fn navigate_to_queue(&mut self) {
        self.queue_cursor = 0;
        self.detail_scroll = 0;
        self.current_screen = Screen::Queue;
    }

    pub fn navigate_to_finish(&mut self) {
        if self.queue.jobs.is_empty() || self.work_active() || !self.queue.all_completed() {
            return;
        }
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
        self.dismiss_stale_cancel_confirm();
        self.finish_cursor = 0;
        self.detail_scroll = 0;
        self.current_screen = Screen::Finish;
    }

    /// Drop cancel confirms once the matching work is idle. Enter and y then
    /// no longer cancel finished work after auto-navigation to Finish.
    pub fn dismiss_stale_cancel_confirm(&mut self) {
        let Some((action, _)) = self.confirm_dialog else {
            return;
        };
        let stale = match action {
            ConfirmAction::CancelEncoding => !self.encoding_active,
            ConfirmAction::CancelAnalysis => !self.analysis_active(),
            ConfirmAction::CancelDisc => !self.disc_run_active(),
            _ => false,
        };
        if stale {
            self.confirm_dialog = None;
        }
    }

    pub fn navigate_to_configuration(&mut self) {
        self.send_to_daemon(ClientRequest::SettingsAccess);
        self.send_to_daemon(ClientRequest::SettingsGet);
        self.config_selected = 0;
        self.current_screen = Screen::Configuration;
    }

    /// Whether the live config has diverged from the saved one.
    pub fn config_is_dirty(&self) -> bool {
        self.saved_config != self.config
    }

    pub fn navigate_to_file_confirm(&mut self) {
        self.file_confirm_scroll = 0;
        self.current_screen = Screen::FileConfirm;
    }

    pub fn disc_operation_active(&self) -> bool {
        self.disc_drive_receiver.is_some() || self.disc_run_active()
    }

    /// Whether a disc scan or rip runs here, or any disc work in the attached daemon.
    pub fn disc_run_active(&self) -> bool {
        self.disc_receiver.is_some()
            || matches!(&self.backend, Backend::Remote(remote) if remote.disc_active)
    }

    /// Whether probes run here, or the attached daemon has jobs waiting for them.
    pub fn analysis_active(&self) -> bool {
        self.analysis_receiver.is_some()
            || matches!(&self.backend, Backend::Remote(remote) if self
                .queue
                .jobs
                .iter()
                .take(remote.ids.len())
                .any(|job| matches!(job.status, JobStatus::Analyzing | JobStatus::Pending)))
    }

    pub fn work_active(&self) -> bool {
        self.encoding_active
            || self.analysis_active()
            || self.disc_operation_active()
            || self.folder_scan_receiver.is_some()
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
        self.detail_scroll = 0;
    }

    pub fn queue_move_selected_up(&mut self) {
        if self.is_attached() {
            if let Some(id) = self.daemon_id(self.queue_cursor) {
                self.send_to_daemon(ClientRequest::MoveUp(id));
            }
            return;
        }
        if let Some(index) = self.queue.move_ready_up(self.queue_cursor) {
            self.queue_cursor = index;
            self.queue_list_state.select(Some(index));
            self.detail_scroll = 0;
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
        self.detail_scroll = 0;
    }

    // File explorer

    /// Lists `current_dir`; while attached, asks the daemon and keeps the
    /// current rows until its listing arrives.
    pub fn refresh_dir_entries(&mut self) {
        if self.send_to_daemon(ClientRequest::Browse {
            path: self.current_dir.to_string_lossy().into_owned(),
            show_hidden: true,
        }) {
            return;
        }
        self.dir_entries.clear();

        // Add parent directory
        if let Some(parent) = self.current_dir.parent()
            && parent != self.current_dir
        {
            self.dir_entries.push(Entry::parent());
        }

        // Cached directory rows.
        if let Ok(entries) = std::fs::read_dir(&self.current_dir) {
            let mut found: Vec<Entry> = entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let path = e.path();
                    let is_dir = e.file_type().map_or_else(
                        |_| path.is_dir(),
                        |file_type| file_type.is_dir() || (file_type.is_symlink() && path.is_dir()),
                    );
                    let selectable_disc_image = self.selection_mode == SelectionMode::DiscFolder
                        && crate::disc::is_iso(&path);
                    if !is_dir && !is_video_file(&path) && !selectable_disc_image {
                        return None;
                    }
                    let size = if is_dir {
                        None
                    } else {
                        e.metadata().ok().map(|m| m.len())
                    };
                    Some(Entry { path, is_dir, size })
                })
                .collect();

            // Sort: directories first, then files
            found.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.path.file_name().cmp(&b.path.file_name()),
            });

            self.dir_entries.extend(found);
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

        let entry = self.dir_entries[self.explorer_index].clone();
        if entry.is_parent() || entry.is_dir || !is_video_file(&entry.path) {
            return;
        }
        let selected = entry.path;

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
        if let Some(remote) = self.remote_mut() {
            remote.pending.clear();
            remote.pending_folder = None;
        }
        let batch_start = self.batch_start.min(self.queue.jobs.len());
        if self.selection_mode == SelectionMode::File {
            self.selected_files = self.queue.jobs[batch_start..]
                .iter()
                .map(|j| j.path.clone())
                .collect();
        }
        self.queue.jobs.truncate(batch_start);
        self.current_screen = Screen::FileExplorer;
    }

    pub fn enter_directory(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let entry = self.dir_entries[self.explorer_index].clone();

        if entry.is_parent() {
            if let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                self.refresh_dir_entries();
            }
        } else if entry.is_dir {
            self.current_dir = entry.path;
            self.refresh_dir_entries();
        }
    }

    pub fn select_explorer_entry(&mut self) {
        if self.dir_entries.is_empty() {
            return;
        }

        let entry = self.dir_entries[self.explorer_index].clone();
        let selected = entry.path.clone();

        match self.selection_mode {
            SelectionMode::File => {
                if entry.is_parent() || entry.is_dir {
                    self.enter_directory();
                } else if is_video_file(&selected) {
                    if self.selected_files.is_empty() {
                        // Single file
                        if self.append_jobs(vec![selected]) {
                            self.analyze_jobs();
                        }
                    } else {
                        // Multi-file — include current file and go to confirmation
                        if !self.selected_files.contains(&selected) {
                            self.selected_files.push(selected);
                        }
                        if self.append_jobs(self.selected_files.clone()) {
                            self.navigate_to_file_confirm();
                        }
                    }
                }
            }
            SelectionMode::DiscFolder => {
                // The highlighted folder or image, or the open folder from any
                // other row.
                let target =
                    if !entry.is_parent() && (entry.is_dir || crate::disc::is_iso(&selected)) {
                        selected
                    } else {
                        self.current_dir.clone()
                    };
                self.scan_disc_folder(&target);
            }
            SelectionMode::SettingFolder => {
                let folder = if entry.is_dir && !entry.is_parent() {
                    selected
                } else {
                    self.current_dir.clone()
                };
                let value = folder.to_string_lossy().into_owned();
                self.config_edit_cursor = value.chars().count();
                self.config_edit_buffer = Some(value);
                self.current_screen = Screen::Configuration;
            }
            SelectionMode::Folder | SelectionMode::FolderRecursive => {
                // The highlighted subfolder, or the open folder from any other row.
                let folder = if entry.is_dir && !entry.is_parent() {
                    selected
                } else {
                    self.current_dir.clone()
                };
                let recursive = self.selection_mode == SelectionMode::FolderRecursive;
                self.scan_folder(folder, recursive);
            }
        }
    }

    pub fn scan_folder(&mut self, folder: PathBuf, recursive: bool) {
        if self.folder_scan_receiver.is_some() {
            return;
        }
        let scanning = crate::i18n::t(self.config.language, crate::i18n::Msg::ScanningFiles);
        if let Some(remote) = self.remote_mut() {
            if remote.pending_folder.is_some() {
                return;
            }
            let path = folder.to_string_lossy().into_owned();
            let mode = if recursive {
                "folder_recursive"
            } else {
                "folder"
            };
            remote.pending_folder = Some((path.clone(), mode));
            let _ = remote
                .requests
                .send(ClientRequest::AddPreview { path, mode });
            self.set_info_message(scanning);
            return;
        }
        self.folder_scan_cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel = self.folder_scan_cancel_flag.clone();
        let (tx, rx) = mpsc::channel();
        self.folder_scan_receiver = Some(rx);
        self.set_info_message(crate::i18n::t(
            self.config.language,
            crate::i18n::Msg::ScanningFiles,
        ));

        thread::spawn(move || {
            let result = (|| -> std::io::Result<Vec<PathBuf>> {
                let mut paths = Vec::new();
                if recursive {
                    crate::queue::collect_video_files_cancellable_result(
                        &folder, &mut paths, &cancel,
                    )?;
                } else {
                    for entry in std::fs::read_dir(&folder)? {
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        let path = entry?.path();
                        if is_video_file(&path) {
                            paths.push(path);
                        }
                    }
                }
                paths.sort();
                Ok(paths)
            })()
            .map_err(|error| format!("{}: {error}", folder.display()));
            let _ = tx.send(result);
        });
    }

    /// Whether a folder scan, or a daemon folder preview, is running.
    pub fn folder_scan_active(&self) -> bool {
        self.folder_scan_receiver.is_some()
            || matches!(&self.backend, Backend::Remote(remote) if remote.pending_folder.is_some())
    }

    /// Stops the folder scan; a daemon preview's reply is then ignored.
    pub fn cancel_folder_scan(&mut self) {
        if let Some(remote) = self.remote_mut() {
            if remote.pending_folder.take().is_some() {
                remote.cancelled_previews += 1;
            }
            self.clear_message();
            return;
        }
        self.folder_scan_cancel_flag.store(true, Ordering::Release);
        self.set_info_message(crate::i18n::t(
            self.config.language,
            crate::i18n::Msg::Cancelling,
        ));
    }

    pub fn process_folder_scan(&mut self) {
        let result =
            self.folder_scan_receiver
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(String::new())),
                });
        let Some(result) = result else {
            return;
        };
        self.folder_scan_receiver = None;

        if self.folder_scan_cancel_flag.load(Ordering::Acquire) {
            self.clear_message();
            return;
        }
        let Err(error) = &result else {
            self.accept_scanned_files(result.unwrap_or_default());
            return;
        };
        let failed = crate::i18n::t(self.config.language, crate::i18n::Msg::FolderScanFailed);
        if error.is_empty() {
            self.set_timed_error(failed, 8);
        } else {
            self.set_timed_error(&format!("{failed}: {error}"), 8);
        }
    }

    /// Queue the files a folder scan found: one goes straight to analysis,
    /// several to the confirmation screen.
    fn accept_scanned_files(&mut self, paths: Vec<PathBuf>) {
        self.clear_message();
        if paths.is_empty() {
            let msg = crate::i18n::t(self.config.language, crate::i18n::Msg::NoVideoFiles);
            self.set_message(msg);
        } else if self.append_jobs(paths) {
            if self.queue.jobs.len() - self.batch_start == 1 {
                self.analyze_jobs();
            } else {
                self.navigate_to_file_confirm();
            }
        }
    }

    /// Append `paths` to the queue as a new batch, skipping any already queued
    /// and not finished. Returns whether anything was added; when nothing was,
    /// says so in the status line. While attached, the batch is held as
    /// pending jobs for the daemon.
    fn append_jobs(&mut self, paths: Vec<PathBuf>) -> bool {
        if self.is_attached() {
            let jobs: Vec<EncodingJob> = paths.into_iter().map(EncodingJob::new).collect();
            self.batch_start = self.queue.jobs.len();
            self.queue.jobs.extend(jobs.iter().cloned());
            let added = !jobs.is_empty();
            if let Some(remote) = self.remote_mut() {
                remote.pending = jobs;
            }
            return added;
        }
        self.queue.reset_session_if_finished();
        let resolved = |path: &PathBuf| path.canonicalize().unwrap_or_else(|_| path.clone());
        let queued: std::collections::HashSet<PathBuf> = self
            .queue
            .jobs
            .iter()
            .filter(|job| !job.status.is_terminal())
            .map(|job| resolved(&job.path))
            .collect();
        self.batch_start = self.queue.jobs.len();
        for path in paths {
            if !queued.contains(&resolved(&path)) {
                self.queue.jobs.push(EncodingJob::new(path));
            }
        }
        let added = self.queue.jobs.len() > self.batch_start;
        if !added {
            self.set_timed_message(
                crate::i18n::t(self.config.language, crate::i18n::Msg::WebNothingAdded),
                4,
            );
        }
        added
    }

    /// Probes the current batch; while attached, sends it to the daemon instead.
    fn analyze_jobs(&mut self) {
        if let Some(remote) = self.remote_mut() {
            let pending = std::mem::take(&mut remote.pending);
            let request = match remote.pending_folder.take() {
                Some((path, mode)) => ClientRequest::AddFolder { path, mode },
                None => ClientRequest::Add {
                    paths: pending
                        .iter()
                        .map(|job| job.path.to_string_lossy().into_owned())
                        .collect(),
                },
            };
            let _ = remote.requests.send(request);
            self.queue.jobs.truncate(self.batch_start);
            self.navigate_to_queue();
            return;
        }
        let indices: Vec<usize> = (self.batch_start..self.queue.jobs.len()).collect();
        self.analyze_indices(&indices);
        self.navigate_to_queue();
    }

    /// Probe the jobs at `indices` on a worker thread. Probes already running
    /// keep going: a title that has just finished ripping joins them without
    /// waiting for a round to end.
    fn analyze_indices(&mut self, indices: &[usize]) {
        let non_utf8 = crate::i18n::t(self.config.language, crate::i18n::Msg::NonUtf8Path);
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
                    message: non_utf8.to_string(),
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
            self.analysis_cancel_flag = Arc::new(AtomicBool::new(false));
        }
        let Some(tx) = self.analysis_sender.clone() else {
            return;
        };
        let cancel_flag = self.analysis_cancel_flag.clone();
        self.analysis_outstanding += work.len();

        thread::spawn(move || {
            let paths: Vec<Result<String, AppError>> =
                work.iter().map(|(_, path)| Ok(path.clone())).collect();
            // A panic in the batch is turned into one error per probe.
            let results = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                analyze_batch(&paths, &cancel_flag)
            }))
            .unwrap_or_else(|_| {
                work.iter()
                    .map(|_| Err(AppError::AnalysisThreadPanicked))
                    .collect()
            });
            for ((index, _), result) in work.into_iter().zip(results) {
                let _ = tx.send((index, result));
            }
        });
    }

    /// Apply one completed probe to its job.
    fn apply_analysis_result(&mut self, index: usize, result: Result<AnalysisResult, AppError>) {
        let output_config = self.saved_config.output.clone();
        let track_config = self.saved_config.tracks.clone();
        let audio_config = self.saved_config.audio.clone();
        let lang = self.config.language;
        let Some(job) = self.queue.jobs.get_mut(index) else {
            return;
        };
        if !job.status.awaits_analysis() {
            return;
        }

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
                if job.temporary && job.output_path.is_none() {
                    job.status = JobStatus::Error {
                        message: crate::disc::DiscError::NoDestination.message(lang),
                    };
                    self.queue.error_count += 1;
                    return;
                }
                job.status = JobStatus::AwaitingConfig;
            }
            Err(AppError::Cancelled) => {
                job.status = JobStatus::Skipped {
                    reason: "Cancelled".to_string(),
                };
                self.queue.skipped_count += 1;
                self.queue.cancelled_count += 1;
            }
            Err(e) => {
                job.status = JobStatus::Error {
                    message: e.message(lang),
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
        // A Track Config screen already open keeps its job, its cursors and
        // any open DV dialog; later probe rounds leave it alone.
        if self.current_screen != Screen::TrackConfig {
            self.queue.config_job_index = next_to_configure.unwrap_or(0);
        }

        // Other screens, and an encode already running, keep the screen; the
        // jobs wait on the queue for the user to open them.
        let follow = self.follows_background_work() && !self.encoding_active;
        if next_to_configure.is_some() {
            if follow && self.current_screen != Screen::TrackConfig {
                self.navigate_to_track_config();
            }
        } else if follow && self.disc_receiver.is_none() {
            // A rip still running has more titles to add to this queue.
            self.navigate_to_finish();
        }
        self.dismiss_stale_cancel_confirm();
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

    /// Detach the current analysis round: late probe results are dropped.
    fn drop_analysis_round(&mut self) {
        self.analysis_cancel_flag.store(true, Ordering::Release);
        self.analysis_receiver = None;
        self.analysis_sender = None;
        self.analysis_outstanding = 0;
    }

    /// Cancel every outstanding probe, keeping analysed jobs and concurrent
    /// work. No-op once the analysis round has ended.
    pub fn cancel_analysis(&mut self) {
        if self.send_to_daemon(ClientRequest::CancelAnalysis) {
            return;
        }
        if self.analysis_receiver.is_none() {
            return;
        }
        self.drop_analysis_round();
        for job in &mut self.queue.jobs {
            if matches!(job.status, JobStatus::Analyzing) {
                job.status = JobStatus::Skipped {
                    reason: "Cancelled".to_string(),
                };
                self.queue.skipped_count += 1;
                self.queue.cancelled_count += 1;
            }
        }
        // A batch left with nothing to configure, encode or show, and no
        // ripped title, goes home.
        let nothing_left = self.queue.jobs.iter().all(|job| {
            !job.temporary
                && matches!(
                    job.status,
                    JobStatus::Skipped { .. } | JobStatus::Error { .. }
                )
        });
        if nothing_left && !self.work_active() {
            self.clear_queue();
            self.navigate_to_home();
        } else {
            self.finish_analysis_round();
        }
    }

    // Track configuration

    pub fn current_config_job(&self) -> Option<&EncodingJob> {
        self.queue.jobs.get(self.queue.config_job_index)
    }

    /// The job at `config_job_index`, while it still accepts track changes.
    pub fn current_config_job_mut(&mut self) -> Option<&mut EncodingJob> {
        if !self.is_track_configurable(self.queue.config_job_index) {
            return None;
        }
        self.queue.jobs.get_mut(self.queue.config_job_index)
    }

    /// Whether the job at `index` accepts track changes: awaiting them, or
    /// ready and not being encoded.
    pub fn is_track_configurable(&self, index: usize) -> bool {
        self.queue
            .jobs
            .get(index)
            .is_some_and(|job| match job.status {
                JobStatus::AwaitingConfig => true,
                JobStatus::Ready => !self.encoding_session_indices.contains(&index),
                _ => false,
            })
    }

    pub fn confirm_track_config(&mut self) {
        if !self.is_track_configurable(self.queue.config_job_index) {
            return;
        }
        if self.is_attached() {
            self.send_track_selection(false);
            return;
        }
        self.queue.jobs[self.queue.config_job_index].status = JobStatus::Ready;
        if !self.encoding_active {
            self.start_encoding();
        }
        self.advance_track_config();
    }

    /// Open the next job awaiting its tracks, wrapping around to earlier
    /// jobs, or the queue when none is left.
    fn advance_track_config(&mut self) {
        let len = self.queue.jobs.len();
        let next_index = (1..=len)
            .map(|k| (self.queue.config_job_index + k) % len)
            .find(|&i| matches!(self.queue.jobs[i].status, JobStatus::AwaitingConfig));

        if let Some(idx) = next_index {
            self.queue.config_job_index = idx;
            self.reset_track_config_cursor();
        } else {
            self.detail_scroll = 0;
            self.navigate_to_queue();
        }
    }

    /// Sends the current job's track choices to the daemon, once per reply.
    fn send_track_selection(&mut self, apply_to_remaining: bool) {
        let index = self.queue.config_job_index;
        let id = self.daemon_id(index);
        let (Some(id), Some(job), Backend::Remote(remote)) =
            (id, self.queue.jobs.get(index), &mut self.backend)
        else {
            return;
        };
        if remote.tracks_in_flight.is_some() {
            return;
        }
        let selection = &job.track_selection;
        let body = json!({
            "id": id,
            "audio_indices": selection.audio_indices,
            "audio_to_opus": selection.audio_to_opus,
            "subtitle_indices": selection.subtitle_indices,
            "remux_only": job.remux_only,
            "dv_mode": job.dv_mode.map(crate::daemon::api::dv_mode_name),
            "apply_to_remaining": apply_to_remaining,
        });
        remote.tracks_in_flight = Some((id, apply_to_remaining));
        let _ = remote.requests.send(ClientRequest::SetTracks(body));
    }

    /// Apply the current job's track choices to every job still awaiting its
    /// tracks, matching tracks by position, then confirm the current job.
    pub fn apply_track_config_to_remaining(&mut self) {
        let index = self.queue.config_job_index;
        if !self.is_track_configurable(index) {
            return;
        }
        if self.is_attached() {
            self.send_track_selection(true);
            return;
        }
        let job = &self.queue.jobs[index];
        let current_awaiting = matches!(job.status, JobStatus::AwaitingConfig);
        let (audio_modes, subtitle_selected) = crate::queue::job::track_choices(job);
        let dv_mode = job.dv_mode;
        let source_dv_profile = job.metadata.as_ref().and_then(|meta| meta.dv_profile);
        let applied = crate::queue::job::apply_to_remaining(
            &mut self.queue.jobs,
            &audio_modes,
            &subtitle_selected,
            dv_mode,
            source_dv_profile,
            &self.saved_config.output,
            self.saved_config.encoder,
        );
        make_output_paths_unique(&mut self.queue.jobs);
        let configured = applied - usize::from(current_awaiting) + 1;
        self.confirm_track_config();
        let message = crate::i18n::t(self.config.language, crate::i18n::Msg::WebTracksApplied)
            .replace("{n}", &configured.to_string());
        self.set_timed_success(&message, 4);
    }

    /// Abandon every job not yet finished, keeping finished ones, and return
    /// to the queue, or to Home when nothing is left. While attached, the
    /// jobs stay in the daemon and the queue opens.
    pub fn cancel_track_config(&mut self) {
        if self.encoding_active || self.disc_operation_active() || self.is_attached() {
            self.navigate_to_queue();
            return;
        }
        self.drop_analysis_round();
        let root = crate::disc::staging::staging_root(&self.config);
        for job in &self.queue.jobs {
            if job.temporary && !job.status.is_terminal() {
                crate::disc::staging::discard_staged(&root, &job.path);
            }
        }
        self.queue.jobs.retain(|job| job.status.is_terminal());
        if self.queue.jobs.is_empty() {
            self.clear_queue();
            self.navigate_to_home();
        } else {
            self.queue.config_job_index = 0;
            self.navigate_to_queue();
        }
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

            let configurable = self.is_track_configurable(idx);
            if configurable {
                self.queue.config_job_index = idx;
                self.reset_track_config_cursor();
                return;
            }
        }
    }

    // Encoding

    #[allow(clippy::too_many_lines)]
    pub fn start_encoding(&mut self) {
        if self.is_attached() {
            return;
        }
        if let Some(pid) = (self.daemon_running)() {
            warn!("Not encoding locally: the daemon runs as PID {pid}");
            let msg = crate::i18n::t(
                self.config.language,
                crate::i18n::Msg::DaemonStartedElsewhere,
            );
            self.set_message(msg);
            return;
        }
        info!("Starting encoding process");
        let follow_active = self.current_screen != Screen::Queue
            || self.queue_cursor == self.queue.current_job_index;

        let output_config = self.saved_config.output.clone();
        let audio_config = self.saved_config.audio.clone();

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
                    tracks: crate::tracks::OutputTracks {
                        external_subtitles: selected_subs
                            .iter()
                            .filter(|track| track.external.is_some())
                            .cloned()
                            .collect(),
                        ..j.track_selection
                            .resolve_for(&j.audio_tracks, &audio_config, &output)
                    },
                    input: j.path.clone(),
                    output,
                    source_identity,
                    metadata,
                    dv_mode: j.dv_mode.unwrap_or_default(),
                    remux_only: j.remux_only,
                })
            })
            .take(1)
            .collect();

        info!("Jobs to encode: {}", worker_jobs.len());

        // Mirror the daemon: an empty session is never armed. Temporary Ready
        // jobs without a destination are skipped by the filter_map above.
        if worker_jobs.is_empty() {
            let lang = self.config.language;
            for job in &mut self.queue.jobs {
                if matches!(job.status, JobStatus::Ready)
                    && job.temporary
                    && job.output_path.is_none()
                {
                    job.status = JobStatus::Error {
                        message: crate::disc::DiscError::NoDestination.message(lang),
                    };
                    self.queue.error_count += 1;
                }
            }
            self.encoding_active = false;
            self.progress_receiver = None;
            self.encoding_session_indices.clear();
            return;
        }

        self.encoding_active = true;
        self.tui_lock = match crate::daemon::lifecycle::write_tui_lock() {
            Ok(guard) => Some(guard),
            Err(e) => {
                warn!("Could not write the TUI lock file: {e}");
                None
            }
        };
        self.cancel_flag = Arc::new(AtomicBool::new(false));

        let (tx, rx) = mpsc::channel();
        self.progress_receiver = Some(rx);

        self.encoding_session_indices = worker_jobs.iter().map(|job| job.index).collect();
        if let Some(&index) = self.encoding_session_indices.first() {
            self.queue.current_job_index = index;
            if follow_active {
                self.queue_cursor = index;
                self.queue_list_state.select(Some(index));
            }
        }

        let ready = self
            .queue
            .jobs
            .iter()
            .filter(|job| matches!(job.status, JobStatus::Ready))
            .count();
        self.queue.total_jobs_to_encode = self.queue.encoding_progress_done + ready;
        // The elapsed clock counts encoding time only; the idle gap since the
        // last session ended is shifted out of it.
        let now = std::time::Instant::now();
        match (self.queue.start_time, self.queue.end_time) {
            (Some(start), Some(end)) => {
                self.queue.start_time = Some(start + now.duration_since(end));
            }
            (None, _) => self.queue.start_time = Some(now),
            (Some(_), None) => {}
        }
        self.queue.end_time = None;
        if let Some(&index) = self.encoding_session_indices.first()
            && let Some(job) = self.queue.jobs.get_mut(index)
        {
            job.status = JobStatus::Encoding { progress: 0.0 };
        }

        let cancel_flag = self.cancel_flag.clone();
        let config = self.saved_config.clone();

        thread::spawn(move || {
            run_worker(worker_jobs, &config, &cancel_flag, &tx);
        });
    }

    pub fn cancel_encoding(&mut self) {
        if self.send_to_daemon(ClientRequest::Cancel) {
            return;
        }
        self.cancel_flag.store(true, Ordering::Release);
        let ready = self
            .queue
            .jobs
            .iter()
            .filter(|job| matches!(job.status, JobStatus::Ready))
            .count();
        if self.encoding_active {
            self.queue.total_jobs_to_encode = self.queue.encoding_progress_done + 1 + ready;
        }
        for job in &mut self.queue.jobs {
            if matches!(job.status, JobStatus::Ready) {
                job.status = JobStatus::Skipped {
                    reason: "Cancelled".to_string(),
                };
                self.queue.skipped_count += 1;
                self.queue.cancelled_count += 1;
                self.queue.encoding_progress_done += 1;
            }
        }
    }

    // Disc ripping

    /// Start drive discovery and open the drive-selection screen.
    pub fn start_disc_flow(&mut self) {
        if self.disc_operation_active() {
            self.navigate_to_queue();
            return;
        }
        if self.send_to_daemon(ClientRequest::DiscsList) {
            self.disc_state = DiscState::Discovering;
            self.disc_drives.clear();
            self.disc_drive_cursor = 0;
            self.disc_drive_list_state.select(Some(0));
            self.current_screen = Screen::DiscDrives;
            return;
        }
        let lang = self.config.language;
        let bin = match crate::disc::find_makemkvcon(&self.config) {
            Ok(bin) => bin,
            Err(e) => {
                self.set_timed_error(&e.message(lang), 8);
                return;
            }
        };

        self.disc_cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel = self.disc_cancel_flag.clone();
        let (tx, rx) = mpsc::channel();
        self.disc_drive_receiver = Some(rx);
        self.disc_state = DiscState::Discovering;
        self.disc_drives.clear();
        self.disc_drive_cursor = 0;
        self.disc_drive_list_state.select(Some(0));
        self.current_screen = Screen::DiscDrives;

        thread::spawn(move || {
            let _ = tx.send(crate::disc::list_drives(&bin, &cancel));
        });
    }

    pub fn process_disc_drive_events(&mut self) {
        let result =
            self.disc_drive_receiver
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(DiscError::Failed(
                        crate::i18n::t(
                            self.config.language,
                            crate::i18n::Msg::DriveDiscoveryStopped,
                        )
                        .to_string(),
                    ))),
                });
        let Some(result) = result else {
            return;
        };
        self.disc_drive_receiver = None;

        if self.disc_state == DiscState::Cancelling {
            self.disc_state = DiscState::Ready;
            self.navigate_to_home();
            return;
        }

        match result {
            Ok(drives) => {
                self.disc_drives = drives;
                self.clear_message();
            }
            Err(DiscError::NoDrive) => {
                self.disc_drives.clear();
                self.set_message(crate::i18n::t(
                    self.config.language,
                    crate::i18n::Msg::DiscNoDrive,
                ));
            }
            Err(error) => {
                self.disc_drives.clear();
                self.set_timed_error(&error.message(self.config.language), 8);
            }
        }
        self.disc_state = DiscState::Ready;
        // A single drive is scanned without asking.
        if self.disc_drives.len() == 1 {
            self.scan_disc(0);
        }
    }

    /// Start scanning the drive at `index`, or open the file explorer for the
    /// folder row that follows the last drive.
    pub fn scan_disc(&mut self, index: usize) {
        self.clear_message();
        if index == self.disc_drives.len() {
            self.selection_mode = SelectionMode::DiscFolder;
            self.refresh_dir_entries();
            self.current_screen = Screen::FileExplorer;
            return;
        }
        let Some(drive) = self.disc_drives.get(index).cloned() else {
            return;
        };
        if self.is_attached() {
            self.request_disc_scan(json!({"drive": drive.id}));
            return;
        }
        self.begin_disc_scan(DiscSource::Drive(drive));
    }

    /// Scan a ripped disc folder or ISO image picked in the explorer.
    pub fn scan_disc_folder(&mut self, path: &Path) {
        if self.is_attached() {
            self.request_disc_scan(json!({"folder": path.to_string_lossy()}));
            return;
        }
        match DiscSource::folder(path) {
            Ok(source) => self.begin_disc_scan(source),
            Err(e) => self.set_timed_error(&e.message(self.config.language), 8),
        }
    }

    /// Asks the daemon to scan the disc `body` names and shows the title screen.
    fn request_disc_scan(&mut self, body: Value) {
        self.send_to_daemon(ClientRequest::DiscsScan(body));
        self.disc_titles.clear();
        self.disc_selected.clear();
        self.disc_cursor = 0;
        self.disc_list_state.select(Some(0));
        self.disc_state = DiscState::Scanning;
        self.current_screen = Screen::DiscTitles;
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
        let _scan = crate::disc::worker::spawn_scan(bin, source, &self.disc_cancel_flag, tx);
        self.current_screen = Screen::DiscTitles;
    }

    pub fn disc_move_up(&mut self) {
        let cursor = if self.current_screen == Screen::DiscDrives {
            &mut self.disc_drive_cursor
        } else {
            &mut self.disc_cursor
        };
        *cursor = cursor.saturating_sub(1);
        self.detail_scroll = 0;
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
        self.detail_scroll = 0;
        self.sync_disc_list_state();
    }

    fn sync_disc_list_state(&mut self) {
        self.disc_drive_list_state
            .select(Some(self.disc_drive_cursor));
        self.disc_list_state.select(Some(self.disc_cursor));
    }

    /// Mark or unmark the title under the cursor.
    pub fn toggle_disc_title(&mut self) {
        self.clear_message();
        let Some(title) = self.disc_titles.get(self.disc_cursor) else {
            return;
        };
        if let Some(pos) = self.disc_selected.iter().position(|id| *id == title.id) {
            self.disc_selected.remove(pos);
        } else {
            self.disc_selected.push(title.id);
        }
    }

    /// Select every title, or clear the selection when all are selected.
    pub fn toggle_all_disc_titles(&mut self) {
        self.clear_message();
        if self.disc_selected.len() == self.disc_titles.len() {
            self.disc_selected.clear();
        } else {
            self.disc_selected = self.disc_titles.iter().map(|title| title.id).collect();
        }
    }

    /// Queue every marked title and start extracting them.
    pub fn start_disc_rip(&mut self) {
        let lang = self.config.language;
        if self.disc_state != DiscState::Ready || self.disc_receiver.is_some() {
            return;
        }
        if self.disc_selected.is_empty() {
            self.set_message(crate::i18n::t(lang, crate::i18n::Msg::DiscNothingSelected));
            return;
        }
        if self.is_attached() {
            let body = match &self.disc_source {
                Some(DiscSource::Drive(drive)) => {
                    json!({"drive": drive.id, "titles": self.disc_selected})
                }
                Some(DiscSource::Folder(path)) => {
                    json!({"folder": path.to_string_lossy(), "titles": self.disc_selected})
                }
                None => return,
            };
            self.send_to_daemon(ClientRequest::DiscsRip(body));
            self.navigate_to_queue();
            return;
        }
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

        // Each title is a queue job from the start, renders in the queue
        // screen and shares its cancellation. The path is the title's name
        // until the file it extracts to is known.
        self.queue.reset_session_if_finished();
        self.disc_job_indices.clear();
        for title in &titles {
            let mut job = EncodingJob::new(PathBuf::from(title.name.clone()));
            job.status = JobStatus::Ripping { progress: 0.0 };
            job.temporary = true;
            self.disc_job_indices.push(self.queue.jobs.len());
            self.queue.jobs.push(job);
        }

        self.disc_cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        self.disc_receiver = Some(rx);
        let _rip = crate::disc::worker::spawn_rips(
            bin,
            self.config.clone(),
            source,
            titles,
            &self.disc_cancel_flag,
            tx,
        );
        self.navigate_to_queue();
    }

    /// Esc: cancel whatever is running and step back one screen. A blocked
    /// daemon is left running and the screen steps back.
    pub fn leave_disc_screen(&mut self) {
        self.clear_message();
        if self.disc_operation_active() && !self.daemon_blocked() {
            self.cancel_disc_operation();
            return;
        }
        match self.current_screen {
            Screen::DiscTitles => self.current_screen = Screen::DiscDrives,
            _ => self.navigate_to_home(),
        }
    }

    pub fn cancel_disc_operation(&mut self) {
        if self.disc_operation_active() {
            if !self.send_to_daemon(ClientRequest::DiscsCancel) {
                self.disc_cancel_flag.store(true, Ordering::Release);
            }
            self.disc_state = DiscState::Cancelling;
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
                DiscEvent::TitlesFound(_) if self.disc_state == DiscState::Cancelling => {
                    self.finish_disc_cancellation();
                }
                DiscEvent::TitlesFound(scan) => {
                    self.disc_titles = scan.titles;
                    self.disc_state = DiscState::Ready;
                    self.disc_receiver = None;
                }
                DiscEvent::Ripping { index, progress } => {
                    if let Some(&job_index) = self.disc_job_indices.get(index) {
                        if let Some(job) = self.queue.jobs.get_mut(job_index)
                            && matches!(job.status, JobStatus::Ripping { .. })
                        {
                            job.status = JobStatus::Ripping { progress };
                        }
                        // A concurrent encode owns `current_job_index` and the
                        // cursor-follow; the rip row only claims them when no
                        // encode is running.
                        if !self.encoding_active {
                            if self.queue_cursor == self.queue.current_job_index {
                                self.queue_cursor = job_index;
                                self.queue_list_state.select(Some(job_index));
                            }
                            self.queue.current_job_index = job_index;
                        }
                    }
                }
                // Probing starts now, while the drive moves on to the next
                // title; peak disk holds at one rip plus one encode input.
                DiscEvent::TitleReady { index, path } => {
                    if let Some(&job_index) = self.disc_job_indices.get(index)
                        && let Some(job) = self.queue.jobs.get_mut(job_index)
                        && matches!(job.status, JobStatus::Ripping { .. })
                    {
                        job.source_size = std::fs::metadata(&path).ok().map(|m| m.len());
                        job.path = path;
                        job.status = JobStatus::Pending;
                        self.analyze_indices(&[job_index]);
                    }
                }
                DiscEvent::Error { index, error } => {
                    self.disc_receiver = None;
                    self.fail_disc_run(index, &error.message(lang));
                }
                DiscEvent::Cancelled => {
                    self.finish_disc_cancellation();
                }
                DiscEvent::Finished => {
                    self.disc_receiver = None;
                    self.settle_after_rips();
                    self.dismiss_stale_cancel_confirm();
                }
            }
        }

        if worker_gone && self.disc_receiver.is_some() {
            self.disc_receiver = None;
            if self.disc_state == DiscState::Cancelling {
                self.finish_disc_cancellation();
            } else {
                let message = crate::disc::DiscError::Failed(
                    crate::i18n::t(lang, crate::i18n::Msg::DiscRunStopped).to_string(),
                )
                .message(lang);
                // The failure lands on the title that was being extracted:
                // the first job still in `Ripping`.
                let failed = self
                    .disc_job_indices
                    .iter()
                    .position(|&job_index| {
                        self.queue
                            .jobs
                            .get(job_index)
                            .is_some_and(|job| matches!(job.status, JobStatus::Ripping { .. }))
                    })
                    .unwrap_or(0);
                self.fail_disc_run(failed, &message);
            }
        }
    }

    /// Complete a cancelled title scan or rip run.
    fn finish_disc_cancellation(&mut self) {
        let return_to_drives = self.current_screen == Screen::DiscTitles;
        self.disc_receiver = None;
        self.disc_state = DiscState::Ready;
        self.skip_remaining_rips();
        if return_to_drives {
            self.current_screen = Screen::DiscDrives;
        } else {
            self.settle_after_rips();
        }
        self.dismiss_stale_cancel_confirm();
    }

    /// Report a failure where the user is looking: on the title screen while
    /// scanning, on the queue once titles are being extracted.
    fn fail_disc_run(&mut self, index: usize, message: &str) {
        if self.disc_state == DiscState::Cancelling {
            self.finish_disc_cancellation();
            return;
        }
        if matches!(self.disc_state, DiscState::Scanning)
            || self.current_screen == Screen::DiscTitles
        {
            self.disc_state = DiscState::Failed(message.to_string());
            return;
        }
        self.disc_state = DiscState::Ready;
        self.fail_remaining_rips(index, message);
        self.settle_after_rips();
    }

    /// Record the failure on the title that hit it, and close out the ones
    /// behind it: the worker stops at the first failure.
    fn fail_remaining_rips(&mut self, index: usize, message: &str) {
        let failed = self.disc_job_indices.get(index).copied();
        for &job_index in &self.disc_job_indices {
            let Some(job) = self.queue.jobs.get_mut(job_index) else {
                continue;
            };
            if !matches!(job.status, JobStatus::Ripping { .. }) {
                continue;
            }
            if Some(job_index) == failed {
                job.status = JobStatus::Error {
                    message: message.to_string(),
                };
                self.queue.error_count += 1;
            } else {
                job.status = JobStatus::Skipped {
                    reason: message.to_string(),
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

    /// Open track configuration for the highlighted job, or the first that still needs it.
    pub fn configure_next_job(&mut self) {
        let cursor = self.queue_cursor;
        let index = self
            .is_track_configurable(cursor)
            .then_some(cursor)
            .or_else(|| {
                self.queue
                    .jobs
                    .iter()
                    .position(|job| matches!(job.status, JobStatus::AwaitingConfig))
            });
        let Some(index) = index else {
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
                self.queue.cancelled_count += 1;
            }
        }
    }

    /// The rip run is over. Probes for the titles it produced may still be
    /// running, and they carry the queue on from here.
    fn settle_after_rips(&mut self) {
        if self.analysis_receiver.is_none()
            && self.follows_background_work()
            && self.current_screen != Screen::TrackConfig
        {
            self.navigate_to_queue();
        }
    }

    #[allow(clippy::too_many_lines)]
    /// Mark a session job as successfully finished and record the output size.
    fn finish_job(&mut self, idx: usize, status: JobStatus) {
        let Some(job) = self.queue.jobs.get_mut(idx) else {
            return;
        };
        job.status = status;
        if let Some(ref output_path) = job.output_path {
            job.output_size = std::fs::metadata(output_path).ok().map(|m| m.len());
        }
        self.queue.converted_count += 1;
        self.queue.encoding_progress_done += 1;
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

        let mut session_finished = false;

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
                    self.finish_job(idx, JobStatus::Done);
                }
                WorkerMessage::DoneWithVmaf(idx, score) => {
                    self.finish_job(idx, JobStatus::DoneWithVmaf { score });
                }
                WorkerMessage::Error(idx, msg) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::Error { message: msg };
                        self.queue.error_count += 1;
                        self.queue.encoding_progress_done += 1;
                    }
                }
                WorkerMessage::QualityWarning(idx, vmaf, min_score, threshold) => {
                    self.finish_job(
                        idx,
                        JobStatus::QualityWarning {
                            vmaf,
                            min_score,
                            threshold,
                        },
                    );
                }
                WorkerMessage::Verifying(idx) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.status = JobStatus::Verifying;
                    }
                }
                WorkerMessage::DoneVmafFailed(idx, reason) => {
                    self.finish_job(idx, JobStatus::DoneVmafFailed { reason });
                }
                WorkerMessage::SourceDeleted(idx) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.source_deleted = true;
                    }
                }
                WorkerMessage::SourceKeptLowVmaf(idx, vmaf, _min) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.source_kept_vmaf = Some(vmaf);
                    }
                }
                WorkerMessage::SourceKept(idx, reason) => {
                    if let Some(job) = self.queue.jobs.get_mut(idx) {
                        job.source_kept_reason = Some(reason);
                    }
                }
                WorkerMessage::Cancelled => {
                    for &index in &self.encoding_session_indices {
                        if let Some(job) = self.queue.jobs.get_mut(index)
                            && !job.status.is_terminal()
                        {
                            job.status = JobStatus::Skipped {
                                reason: "Cancelled".to_string(),
                            };
                            self.queue.skipped_count += 1;
                            self.queue.cancelled_count += 1;
                            self.queue.encoding_progress_done += 1;
                        }
                    }
                    self.encoding_active = false;
                    session_finished = true;
                }
                WorkerMessage::Finished => {
                    self.encoding_active = false;
                    session_finished = true;
                }
            }
        }

        // The worker is gone; close out whatever it left unfinished.
        if worker_gone && self.encoding_active {
            let stopped = crate::i18n::t(self.config.language, crate::i18n::Msg::EncodingStopped);
            self.progress_receiver = None;
            for &index in &self.encoding_session_indices {
                if let Some(job) = self.queue.jobs.get_mut(index)
                    && !job.status.is_terminal()
                {
                    job.status = JobStatus::Error {
                        message: stopped.to_string(),
                    };
                    self.queue.error_count += 1;
                    self.queue.encoding_progress_done += 1;
                }
            }
            self.encoding_active = false;
            session_finished = true;
        }

        crate::disc::staging::cleanup_finished(
            &crate::disc::staging::staging_root(&self.config),
            &mut self.queue.jobs,
        );

        if session_finished {
            self.tui_lock = None;
            self.progress_receiver = None;
            self.encoding_session_indices.clear();
            // Set on every session end; the next session start subtracts the
            // idle gap from the elapsed clock.
            self.queue.end_time = Some(std::time::Instant::now());
            if self.should_quit {
                return;
            }
            if self
                .queue
                .jobs
                .iter()
                .any(|job| matches!(job.status, JobStatus::Ready))
            {
                self.start_encoding();
            } else if self.disc_receiver.is_none()
                && self.analysis_receiver.is_none()
                && self.queue.all_completed()
            {
                self.queue.end_time = Some(std::time::Instant::now());
                if self.follows_background_work() {
                    self.navigate_to_finish();
                }
            }
            self.dismiss_stale_cancel_confirm();
        }
    }

    /// Delete the staging directory of every ripped title in the queue; does
    /// nothing while attached.
    pub fn discard_staged_jobs(&self) {
        if self.is_attached() {
            return;
        }
        let root = crate::disc::staging::staging_root(&self.config);
        for job in self.queue.jobs.iter().filter(|job| job.temporary) {
            crate::disc::staging::discard_staged(&root, &job.path);
        }
    }

    /// Whether the job at `index` can leave the queue: waiting or finished,
    /// with no analysis or disc run in flight, and after the job being encoded.
    pub fn can_remove_job(&self, index: usize) -> bool {
        let Some(job) = self.queue.jobs.get(index) else {
            return false;
        };
        let waiting_or_done = matches!(job.status, JobStatus::Ready | JobStatus::AwaitingConfig)
            || job.status.is_terminal();
        waiting_or_done
            && !self.analysis_active()
            && !self.disc_operation_active()
            && (!self.encoding_active || index > self.queue.current_job_index)
    }

    /// Remove the job at `index`, deleting a ripped title's staging files.
    pub fn remove_job(&mut self, index: usize) {
        if self.is_attached() {
            if let Some(id) = self.daemon_id(index) {
                self.send_to_daemon(ClientRequest::Remove(id));
            }
            return;
        }
        if !self.can_remove_job(index) {
            return;
        }
        let job = self.queue.jobs.remove(index);
        if let Some(change) = job.size_change() {
            self.queue.cleared_saved_bytes = self.queue.cleared_saved_bytes.saturating_add(change);
        }
        if matches!(job.status, JobStatus::Ready) {
            let remaining_ready = self
                .queue
                .jobs
                .iter()
                .filter(|job| matches!(job.status, JobStatus::Ready))
                .count();
            self.queue.total_jobs_to_encode = self.queue.encoding_progress_done
                + usize::from(self.encoding_active)
                + remaining_ready;
        }
        if job.temporary {
            crate::disc::staging::discard_staged(
                &crate::disc::staging::staging_root(&self.config),
                &job.path,
            );
        }
        if self.queue.config_job_index > index {
            self.queue.config_job_index -= 1;
        }
        if self.queue.current_job_index > index {
            self.queue.current_job_index -= 1;
        }
        if self.batch_start > index {
            self.batch_start -= 1;
        }
        self.queue_cursor = self
            .queue_cursor
            .min(self.queue.jobs.len().saturating_sub(1));
        self.queue_list_state.select(Some(self.queue_cursor));
    }

    /// Whether finished jobs can be cleared: nothing runs and one is listed.
    pub fn can_clear_finished(&self) -> bool {
        !self.work_active() && self.queue.jobs.iter().any(|job| job.status.is_terminal())
    }

    /// Remove every finished job while nothing runs, keeping its space saved in
    /// the total and deleting ripped titles' staging files. Returns how many
    /// were removed; while attached, asks the daemon and returns 0.
    pub fn clear_finished(&mut self) -> usize {
        if self.is_attached() {
            let confirm = self
                .queue
                .jobs
                .iter()
                .any(|job| job.temporary && job.status.is_terminal());
            self.send_to_daemon(ClientRequest::ClearFinished(json!({"confirm": confirm})));
            return 0;
        }
        if self.work_active() {
            return 0;
        }
        let root = crate::disc::staging::staging_root(&self.config);
        let before = self.queue.jobs.len();
        let mut cleared_bytes = 0i128;
        for job in self
            .queue
            .jobs
            .iter()
            .filter(|job| job.status.is_terminal())
        {
            cleared_bytes = cleared_bytes.saturating_add(job.size_change().unwrap_or(0));
            if job.temporary {
                crate::disc::staging::discard_staged(&root, &job.path);
            }
        }
        self.queue.cleared_saved_bytes =
            self.queue.cleared_saved_bytes.saturating_add(cleared_bytes);
        self.queue.jobs.retain(|job| !job.status.is_terminal());
        self.queue.config_job_index = 0;
        self.queue.current_job_index = 0;
        self.batch_start = 0;
        self.queue_cursor = self
            .queue_cursor
            .min(self.queue.jobs.len().saturating_sub(1));
        self.queue_list_state.select(Some(self.queue_cursor));
        let removed = before - self.queue.jobs.len();
        let message = crate::i18n::t(self.config.language, crate::i18n::Msg::WebRemovedFinished)
            .replace("{n}", &removed.to_string());
        self.set_timed_success(&message, 3);
        removed
    }

    /// Delete the staging directory of every ripped title in the queue, then
    /// empty the queue.
    fn clear_queue(&mut self) {
        self.discard_staged_jobs();
        self.queue.reset();
    }

    /// Finish screen "New conversion": resets the app; while attached, clears
    /// the daemon's finished jobs like `C` and returns home.
    pub fn new_conversion(&mut self) {
        if !self.is_attached() {
            self.reset();
            return;
        }
        if self
            .queue
            .jobs
            .iter()
            .any(|job| job.temporary && job.status.is_terminal())
        {
            self.confirm_dialog = Some((ConfirmAction::ClearFinishedRips, false));
        } else {
            self.clear_finished();
        }
        self.navigate_to_home();
    }

    pub fn reset(&mut self) {
        self.clear_queue();
        self.encoding_active = false;
        self.tui_lock = None;
        self.selected_files.clear();
        self.progress_receiver = None;
        self.encoding_session_indices.clear();
        self.analysis_cancel_flag = Arc::new(AtomicBool::new(false));
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
                        _ if cancel_flag.load(Ordering::Acquire) => Err(AppError::Cancelled),
                        Ok(path) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let _span =
                                crate::queue::worker::job_span(index, std::path::Path::new(path))
                                    .entered();
                            analyzer::analyze(path, cancel_flag)
                        }))
                        .unwrap_or_else(|_| Err(AppError::AnalysisPanicked(path.clone()))),
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
                .unwrap_or(Err(AppError::AnalysisThreadPanicked))
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn session() -> (
        RemoteSession,
        Receiver<crate::daemon::client::ClientRequest>,
        mpsc::Sender<crate::daemon::client::ClientEvent>,
    ) {
        let (requests, sent) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let session = RemoteSession {
            reachable: false,
            ..RemoteSession::new(requests, events)
        };
        (session, sent, events_tx)
    }

    fn daemon_snapshot(encoding: bool) -> (crate::daemon::client::RemoteSnapshot, Vec<u64>) {
        let mut state = crate::daemon::state::DaemonState::new(AppConfig::default());
        state.instance = 1;
        let mut active = EncodingJob::new(PathBuf::from("/nonexistent/a.mkv"));
        if encoding {
            active.status = JobStatus::Encoding { progress: 37.5 };
        }
        let ids = vec![
            state.queue.push(active),
            state
                .queue
                .push(EncodingJob::new(PathBuf::from("/nonexistent/b.mkv"))),
        ];
        state.encoding_active = encoding;
        let mut snapshot = crate::daemon::client::snapshot_of(state);
        snapshot.taken_ms = 777;
        (snapshot, ids)
    }

    #[test]
    fn a_new_app_runs_locally() {
        assert!(!App::new().is_attached());
    }

    #[test]
    fn attaching_to_an_encoding_daemon_opens_on_its_queue() {
        let (session, sent, _events) = session();
        let (first, ids) = daemon_snapshot(true);

        let app = App::attached(AppConfig::default(), session, &first);

        assert!(app.is_attached());
        assert_eq!(app.current_screen, Screen::Queue);
        assert!(app.encoding_active);
        assert_eq!(app.queue.jobs.len(), 2);
        assert!(matches!(
            app.queue.jobs[0].status,
            JobStatus::Encoding { progress } if (progress - 37.5).abs() < f64::EPSILON
        ));
        assert_eq!(app.queue.jobs[1].path, PathBuf::from("/nonexistent/b.mkv"));
        let Backend::Remote(remote) = &app.backend else {
            panic!("not attached");
        };
        assert_eq!(remote.ids, ids);
        assert_eq!(remote.last_taken_ms, 777);
        assert!(remote.reachable);
        assert!(remote.was_encoding);
        assert!(!remote.host_settings_read_only);
        assert!(app.progress_receiver.is_none());
        assert!(app.analysis_receiver.is_none());
        assert!(app.disc_receiver.is_none());
        assert!(app.tui_lock.is_none());
        assert!(matches!(
            sent.try_recv(),
            Ok(crate::daemon::client::ClientRequest::SettingsAccess)
        ));
        assert!(sent.try_recv().is_err());
    }

    /// An attached app on the idle two-job snapshot; returns it with its request receiver and event sender.
    pub(crate) fn attached_app() -> (
        App,
        Receiver<crate::daemon::client::ClientRequest>,
        mpsc::Sender<crate::daemon::client::ClientEvent>,
    ) {
        let (session, sent, events) = session();
        let (first, _) = daemon_snapshot(false);
        let app = App::attached(AppConfig::default(), session, &first);
        while sent.try_recv().is_ok() {}
        (app, sent, events)
    }

    fn feed(
        app: &mut App,
        events: &mpsc::Sender<crate::daemon::client::ClientEvent>,
        snapshot: crate::daemon::client::RemoteSnapshot,
    ) {
        events
            .send(crate::daemon::client::ClientEvent::Snapshot(Box::new(
                snapshot,
            )))
            .unwrap();
        app.process_client_events();
    }

    pub(crate) fn snapshot_with(
        taken_ms: u64,
        encoding: bool,
        paths: &[&str],
    ) -> crate::daemon::client::RemoteSnapshot {
        let mut state = crate::daemon::state::DaemonState::new(AppConfig::default());
        state.instance = 1;
        for path in paths {
            state.queue.push(EncodingJob::new(PathBuf::from(path)));
        }
        state.encoding_active = encoding;
        let mut snapshot = crate::daemon::client::snapshot_of(state);
        snapshot.taken_ms = taken_ms;
        snapshot
    }

    fn remote(app: &App) -> &RemoteSession {
        let Backend::Remote(remote) = &app.backend else {
            panic!("not attached");
        };
        remote
    }

    #[test]
    fn a_snapshot_replaces_the_queue_and_ids() {
        let (mut app, _sent, events) = attached_app();
        let mut snapshot = snapshot_with(900, false, &["/nonexistent/x.mkv"]);
        snapshot.queue.ids = vec![41];

        feed(&mut app, &events, snapshot);

        assert_eq!(app.queue.jobs.len(), 1);
        assert_eq!(app.queue.jobs[0].path, PathBuf::from("/nonexistent/x.mkv"));
        assert_eq!(remote(&app).ids, vec![41]);
        assert_eq!(remote(&app).last_taken_ms, 900);
    }

    #[test]
    fn an_older_snapshot_is_ignored() {
        let (mut app, _sent, events) = attached_app();
        feed(
            &mut app,
            &events,
            snapshot_with(1200, false, &["/n/new.mkv"]),
        );
        feed(
            &mut app,
            &events,
            snapshot_with(1100, false, &["/n/old.mkv", "/n/old2.mkv"]),
        );

        assert_eq!(app.queue.jobs.len(), 1);
        assert_eq!(app.queue.jobs[0].path, PathBuf::from("/n/new.mkv"));
        assert_eq!(remote(&app).last_taken_ms, 1200);
    }

    #[test]
    fn a_snapshot_keeps_the_track_config_being_edited() {
        let (mut app, _sent, events) = attached_app();
        let paths = ["/nonexistent/a.mkv", "/nonexistent/b.mkv"];
        app.queue.config_job_index = 1;
        app.current_screen = Screen::TrackConfig;
        app.queue.jobs[1].track_selection.audio_indices = vec![3];
        // The daemon moved the edited job to the front.
        let mut snapshot = snapshot_with(900, false, &[paths[1], paths[0]]);
        snapshot.queue.ids.reverse();

        feed(&mut app, &events, snapshot);

        assert_eq!(app.current_screen, Screen::TrackConfig);
        assert_eq!(app.queue.config_job_index, 0);
        assert_eq!(app.queue.jobs[0].path, PathBuf::from(paths[1]));
        assert_eq!(app.queue.jobs[0].track_selection.audio_indices, vec![3]);
        assert_eq!(
            app.queue.jobs[1].track_selection.audio_indices,
            Vec::<usize>::new()
        );
    }

    #[test]
    fn a_removed_job_under_track_config_returns_to_the_queue_with_an_error() {
        let (mut app, _sent, events) = attached_app();
        app.queue.config_job_index = 1;
        app.current_screen = Screen::TrackConfig;
        let mut snapshot = snapshot_with(900, false, &["/nonexistent/a.mkv"]);
        snapshot.queue.ids.truncate(1);

        feed(&mut app, &events, snapshot);

        assert_eq!(app.current_screen, Screen::Queue);
        assert_eq!(
            app.message.as_deref(),
            Some(crate::i18n::t(
                app.config.language,
                crate::i18n::Msg::JobRemovedByDaemon
            ))
        );
        assert_eq!(app.message_kind, MessageKind::Error);
    }

    #[test]
    fn a_request_failure_without_an_http_answer_shows_a_translated_message() {
        let (mut app, _sent, events) = attached_app();
        reply(
            &mut app,
            &events,
            RequestKind::Cancel,
            Err(ClientError::Unreachable("connection refused".into())),
        );
        assert_eq!(
            app.message.as_deref(),
            Some("The daemon did not answer: connection refused")
        );

        app.config.language = crate::config::Language::Italian;
        reply(
            &mut app,
            &events,
            RequestKind::Cancel,
            Err(ClientError::BadResponse("incomplete response".into())),
        );
        assert_eq!(
            app.message.as_deref(),
            Some("Il daemon non ha risposto: incomplete response")
        );
        assert_eq!(app.message_kind, MessageKind::Error);
    }

    #[test]
    fn a_daemon_restart_closes_the_dialogs_that_name_queue_jobs() {
        for action in [
            ConfirmAction::RemoveRip(0),
            ConfirmAction::ClearFinishedRips,
            ConfirmAction::CancelEncoding,
            ConfirmAction::CancelAnalysis,
            ConfirmAction::CancelDisc,
            ConfirmAction::AbandonTrackConfig,
        ] {
            let (mut app, _sent, events) = attached_app();
            app.confirm_dialog = Some((action, true));
            let mut restarted = snapshot_with(5, false, &["/nonexistent/a.mkv"]);
            restarted.instance = remote(&app).instance + 1;

            feed(&mut app, &events, restarted);

            assert_eq!(app.confirm_dialog, None, "{action:?}");
        }
    }

    #[test]
    fn a_daemon_restart_keeps_dialogs_unrelated_to_queue_jobs() {
        let (mut app, _sent, events) = attached_app();
        app.confirm_dialog = Some((ConfirmAction::ExitApp, true));
        let mut restarted = snapshot_with(5, false, &["/nonexistent/a.mkv"]);
        restarted.instance = remote(&app).instance + 1;

        feed(&mut app, &events, restarted);

        assert_eq!(app.confirm_dialog, Some((ConfirmAction::ExitApp, true)));
    }

    #[test]
    fn a_daemon_restart_under_track_config_drops_the_edit_and_returns_to_the_queue() {
        let (mut app, _sent, events) = attached_app();
        app.queue.config_job_index = 0;
        app.queue.jobs[0].track_selection.audio_indices = vec![3];
        app.current_screen = Screen::TrackConfig;
        let mut restarted = snapshot_with(5, false, &["/nonexistent/a.mkv"]);
        restarted.instance = remote(&app).instance + 1;

        feed(&mut app, &events, restarted);

        assert_eq!(app.current_screen, Screen::Queue);
        assert_eq!(
            app.queue.jobs[0].track_selection.audio_indices,
            Vec::<usize>::new()
        );
        assert_eq!(
            app.message.as_deref(),
            Some("The daemon restarted; the track selection was discarded.")
        );
        assert_eq!(app.message_kind, MessageKind::Error);
    }

    #[test]
    fn a_daemon_restart_forgets_the_track_selection_in_flight() {
        let (mut app, _sent, events) = attached_app();
        app.remote_mut().expect("attached").tracks_in_flight = Some((1, true));
        feed(
            &mut app,
            &events,
            snapshot_with(900, false, &["/nonexistent/a.mkv"]),
        );
        assert_eq!(remote(&app).tracks_in_flight, Some((1, true)));

        let mut restarted = snapshot_with(5, false, &["/nonexistent/a.mkv"]);
        restarted.instance = remote(&app).instance + 1;
        feed(&mut app, &events, restarted);

        assert_eq!(remote(&app).tracks_in_flight, None);
    }

    #[test]
    fn a_snapshot_of_the_same_daemon_run_keeps_track_config_open() {
        let (mut app, _sent, events) = attached_app();
        app.queue.config_job_index = 0;
        app.queue.jobs[0].track_selection.audio_indices = vec![3];
        app.current_screen = Screen::TrackConfig;

        feed(
            &mut app,
            &events,
            snapshot_with(900, false, &["/nonexistent/a.mkv"]),
        );

        assert_eq!(app.current_screen, Screen::TrackConfig);
        assert_eq!(app.queue.jobs[0].track_selection.audio_indices, vec![3]);
    }

    #[test]
    fn an_encode_ending_while_attached_opens_finish() {
        let (mut app, _sent, events) = attached_app();
        feed(&mut app, &events, snapshot_with(900, true, &["/n/a.mkv"]));
        app.navigate_to_queue();
        let mut done = snapshot_with(901, false, &["/n/a.mkv"]);
        done.queue.state.jobs[0].status = JobStatus::Done;

        feed(&mut app, &events, done);

        assert_eq!(app.current_screen, Screen::Finish);
        assert!(!remote(&app).was_encoding);
    }

    #[test]
    fn attaching_after_the_encode_ended_does_not_open_finish() {
        let (mut app, _sent, events) = attached_app();
        let mut done = snapshot_with(900, false, &["/n/a.mkv"]);
        done.queue.state.jobs[0].status = JobStatus::Done;

        feed(&mut app, &events, done);

        assert_eq!(app.current_screen, Screen::Home);
    }

    #[test]
    fn a_snapshot_maps_the_daemon_disc_state() {
        let (mut app, _sent, events) = attached_app();
        let cases = [
            (
                Some("no disc"),
                true,
                true,
                true,
                DiscState::Failed("no disc".into()),
            ),
            (None, true, true, true, DiscState::Scanning),
            (None, false, true, true, DiscState::Ready),
            (None, false, false, true, DiscState::Discovering),
            (None, false, false, false, DiscState::Ready),
        ];
        for (taken, (error, scanning, active, listing, expected)) in (1000..).zip(cases) {
            let mut snapshot = snapshot_with(taken, false, &[]);
            snapshot.disc.error = error.map(str::to_string);
            snapshot.disc.scanning = scanning;
            snapshot.disc.active = active;
            snapshot.disc.listing = listing;
            snapshot.disc.titles = vec![DiscTitle {
                id: 7,
                name: "Main".into(),
                duration: std::time::Duration::ZERO,
                size_bytes: 0,
                chapters: 0,
                tracks: Vec::new(),
            }];
            feed(&mut app, &events, snapshot);
            assert_eq!(app.disc_state, expected);
            assert_eq!(app.disc_titles.len(), 1);
            assert_eq!(app.disc_titles[0].id, 7);
        }
    }

    #[test]
    fn a_shrinking_queue_clamps_the_cursors() {
        let (mut app, _sent, events) = attached_app();
        app.queue_cursor = 1;
        app.finish_cursor = 1;

        feed(&mut app, &events, snapshot_with(900, false, &["/n/a.mkv"]));

        assert_eq!(app.queue_cursor, 0);
        assert_eq!(app.finish_cursor, 0);
    }

    #[test]
    fn unreachable_and_reachable_toggle_the_retry_message() {
        let (mut app, _sent, events) = attached_app();
        events
            .send(crate::daemon::client::ClientEvent::Unreachable(
                "refused".into(),
            ))
            .unwrap();
        app.process_client_events();

        assert!(!remote(&app).reachable);
        let retrying = crate::i18n::t(
            app.config.language,
            crate::i18n::Msg::DaemonUnreachableRetrying,
        );
        assert_eq!(app.message.as_deref(), Some(retrying));

        events
            .send(crate::daemon::client::ClientEvent::Reachable)
            .unwrap();
        app.process_client_events();

        assert!(remote(&app).reachable);
        assert!(app.message.is_none());
    }

    #[test]
    fn an_http_error_reply_is_shown() {
        let (mut app, _sent, events) = attached_app();
        events
            .send(crate::daemon::client::ClientEvent::Reply {
                request: crate::daemon::client::RequestKind::Remove,
                result: Err(crate::daemon::client::ClientError::Http {
                    status: 409,
                    error: "job is encoding".into(),
                }),
            })
            .unwrap();

        app.process_client_events();

        assert_eq!(app.message.as_deref(), Some("job is encoding"));
        assert_eq!(app.message_kind, MessageKind::Error);
    }

    #[test]
    fn discarding_staged_jobs_while_attached_keeps_the_daemon_rips() {
        let root = std::env::temp_dir().join(format!("av1c-attached-rips-{}", std::process::id()));
        let file = crate::disc::staging::staged_rip(&root, u32::MAX);
        let (mut app, _sent, _events) = attached_app();
        app.config.disc.staging_directory = Some(root.to_string_lossy().into_owned());
        app.queue.jobs[0].path.clone_from(&file);
        app.queue.jobs[0].temporary = true;

        app.discard_staged_jobs();

        assert!(file.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    use crate::daemon::client::{ClientError, ClientEvent, ClientRequest, RequestKind};
    use serde_json::json;

    fn reply(
        app: &mut App,
        events: &mpsc::Sender<ClientEvent>,
        request: RequestKind,
        result: Result<serde_json::Value, ClientError>,
    ) {
        events.send(ClientEvent::Reply { request, result }).unwrap();
        app.process_client_events();
    }

    fn http_error(error: &str) -> Result<serde_json::Value, ClientError> {
        Err(ClientError::Http {
            status: 409,
            error: error.into(),
        })
    }

    /// An attached app showing two daemon jobs with the ids `[7, 9]`.
    fn attached_with_ids() -> (App, Receiver<ClientRequest>, mpsc::Sender<ClientEvent>) {
        let (mut app, sent, events) = attached_app();
        let mut snapshot = snapshot_with(900, false, &["/n/a.mkv", "/n/b.mkv"]);
        snapshot.queue.ids = vec![7, 9];
        feed(&mut app, &events, snapshot);
        (app, sent, events)
    }

    fn sent_requests(sent: &Receiver<ClientRequest>) -> Vec<ClientRequest> {
        sent.try_iter().collect()
    }

    #[test]
    fn remove_sends_the_id_shown_at_the_cursor() {
        let (mut app, sent, events) = attached_with_ids();
        app.queue_cursor = 1;

        app.remove_job(app.queue_cursor);
        assert_eq!(sent_requests(&sent), vec![ClientRequest::Remove(9)]);
        assert_eq!(app.queue.jobs.len(), 2);

        let mut snapshot = snapshot_with(901, false, &["/n/b.mkv"]);
        snapshot.queue.ids = vec![9];
        feed(&mut app, &events, snapshot);
        app.remove_job(app.queue_cursor);

        assert_eq!(app.queue_cursor, 0);
        assert_eq!(sent_requests(&sent), vec![ClientRequest::Remove(9)]);
    }

    #[test]
    fn the_queue_cursor_follows_its_job_across_snapshots() {
        let (mut app, sent, events) = attached_with_ids();
        app.queue_cursor = 1;

        app.queue_move_selected_up();
        assert_eq!(sent_requests(&sent), vec![ClientRequest::MoveUp(9)]);
        assert_eq!(app.queue_cursor, 1);

        let mut snapshot = snapshot_with(901, false, &["/n/b.mkv", "/n/a.mkv"]);
        snapshot.queue.ids = vec![9, 7];
        feed(&mut app, &events, snapshot);

        assert_eq!(app.queue_cursor, 0);
        assert_eq!(app.queue.jobs[0].path, PathBuf::from("/n/b.mkv"));
    }

    #[test]
    fn a_rip_removal_awaiting_confirmation_follows_its_job() {
        let (mut app, sent, events) = attached_with_ids();
        app.confirm_dialog = Some((ConfirmAction::RemoveRip(1), false));

        let mut reordered = snapshot_with(901, false, &["/n/b.mkv", "/n/a.mkv"]);
        reordered.queue.ids = vec![9, 7];
        feed(&mut app, &events, reordered);
        assert_eq!(
            app.confirm_dialog,
            Some((ConfirmAction::RemoveRip(0), false))
        );

        let mut gone = snapshot_with(902, false, &["/n/a.mkv"]);
        gone.queue.ids = vec![7];
        feed(&mut app, &events, gone);
        assert_eq!(app.confirm_dialog, None);
        assert_eq!(sent_requests(&sent), vec![]);
    }

    #[test]
    fn queue_actions_while_attached_go_to_the_daemon() {
        let (mut app, sent, _events) = attached_with_ids();
        app.encoding_active = true;
        app.queue.jobs[1].status = JobStatus::Ready;

        app.cancel_encoding();
        app.cancel_analysis();

        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::Cancel, ClientRequest::CancelAnalysis]
        );
        assert!(!app.cancel_flag.load(Ordering::Acquire));
        assert!(matches!(app.queue.jobs[1].status, JobStatus::Ready));
    }

    #[test]
    fn clearing_finished_while_attached_confirms_only_shown_rips() {
        let (mut app, sent, events) = attached_with_ids();
        app.queue.jobs[0].status = JobStatus::Done;

        assert_eq!(app.clear_finished(), 0);
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::ClearFinished(json!({"confirm": false}))]
        );
        assert_eq!(app.queue.jobs.len(), 2);

        app.queue.jobs[0].temporary = true;
        app.clear_finished();
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::ClearFinished(json!({"confirm": true}))]
        );

        reply(
            &mut app,
            &events,
            RequestKind::ClearFinished,
            Ok(json!({"removed": 1})),
        );
        assert_eq!(
            app.message.as_deref(),
            Some(
                crate::i18n::t(app.config.language, crate::i18n::Msg::WebRemovedFinished)
                    .replace("{n}", "1")
                    .as_str()
            )
        );
    }

    #[test]
    fn analysis_and_disc_runs_in_the_daemon_count_as_work() {
        let (mut app, _sent, events) = attached_with_ids();
        let mut idle = snapshot_with(901, false, &["/n/a.mkv", "/n/b.mkv"]);
        for job in &mut idle.queue.state.jobs {
            job.status = JobStatus::Ready;
        }
        feed(&mut app, &events, idle);
        assert!(!app.analysis_active());
        assert!(!app.disc_run_active());
        assert!(!app.work_active());

        let mut snapshot = snapshot_with(902, false, &["/n/a.mkv", "/n/b.mkv"]);
        snapshot.queue.state.jobs[0].status = JobStatus::Ready;
        snapshot.queue.state.jobs[1].status = JobStatus::Analyzing;
        snapshot.disc.active = true;
        feed(&mut app, &events, snapshot);

        assert!(app.analysis_active());
        assert!(app.disc_run_active());
        assert!(app.disc_operation_active());
        assert!(app.work_active());
    }

    #[test]
    fn browsing_while_attached_lists_the_daemon_directory() {
        let (mut app, sent, events) = attached_app();
        app.current_dir = PathBuf::from("/srv/media");
        app.selection_mode = SelectionMode::File;

        app.refresh_dir_entries();
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::Browse {
                path: "/srv/media".into(),
                show_hidden: true,
            }]
        );

        reply(
            &mut app,
            &events,
            RequestKind::Browse,
            Ok(json!({
                "path": "/srv/media",
                "parent": "/srv",
                "dirs": [{"name": "Shows", "symlink": false}],
                "files": [
                    {"name": "a.mkv", "size": 12, "is_video": true, "is_iso": false},
                    {"name": "disc.iso", "size": 5, "is_video": false, "is_iso": true},
                    {"name": "notes.txt", "size": 1, "is_video": false, "is_iso": false},
                ],
            })),
        );

        assert_eq!(app.current_dir, PathBuf::from("/srv/media"));
        assert_eq!(
            app.dir_entries,
            vec![
                Entry::parent(),
                Entry {
                    path: PathBuf::from("/srv/media/Shows"),
                    is_dir: true,
                    size: None,
                },
                Entry {
                    path: PathBuf::from("/srv/media/a.mkv"),
                    is_dir: false,
                    size: Some(12),
                },
            ]
        );
    }

    #[test]
    fn a_refused_browse_falls_back_to_the_daemon_start_folder() {
        let (mut app, sent, events) = attached_app();
        app.current_dir = PathBuf::from("/outside");
        app.refresh_dir_entries();
        let _ = sent_requests(&sent);

        reply(
            &mut app,
            &events,
            RequestKind::Browse,
            http_error("outside the browse root"),
        );
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::Browse {
                path: String::new(),
                show_hidden: true,
            }]
        );

        reply(&mut app, &events, RequestKind::Browse, http_error("no"));
        assert_eq!(sent_requests(&sent), vec![]);
    }

    #[test]
    fn a_folder_pick_previews_and_fills_file_confirm() {
        let (mut app, sent, events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::Folder;

        app.scan_folder(PathBuf::from("/srv/media"), false);
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::AddPreview {
                path: "/srv/media".into(),
                mode: "folder",
            }]
        );
        assert!(app.folder_scan_receiver.is_none());

        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Ok(json!({"files": ["/srv/media/a.mkv", "/srv/media/b.mkv"]})),
        );

        assert_eq!(app.current_screen, Screen::FileConfirm);
        assert_eq!(app.batch_start, 2);
        let batch: Vec<PathBuf> = app.queue.jobs[app.batch_start..]
            .iter()
            .map(|job| job.path.clone())
            .collect();
        assert_eq!(
            batch,
            vec![
                PathBuf::from("/srv/media/a.mkv"),
                PathBuf::from("/srv/media/b.mkv")
            ]
        );
        assert_eq!(remote(&app).pending.len(), 2);
        assert_eq!(sent_requests(&sent), vec![]);
    }

    /// An attached app on File Confirm with the folder batch `/srv/media/{a,b}.mkv`.
    fn previewed_folder() -> (App, Receiver<ClientRequest>, mpsc::Sender<ClientEvent>) {
        let (mut app, sent, events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::Folder;
        app.scan_folder(PathBuf::from("/srv/media"), false);
        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Ok(json!({"files": ["/srv/media/a.mkv", "/srv/media/b.mkv"]})),
        );
        let _ = sent_requests(&sent);
        (app, sent, events)
    }

    #[test]
    fn a_cancelled_preview_reply_is_never_shown_as_the_next_folders_files() {
        let (mut app, sent, events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::Folder;
        app.scan_folder(PathBuf::from("/A"), false);
        app.cancel_folder_scan();
        app.scan_folder(PathBuf::from("/B"), false);
        assert_eq!(
            sent_requests(&sent),
            vec![
                ClientRequest::AddPreview {
                    path: "/A".into(),
                    mode: "folder"
                },
                ClientRequest::AddPreview {
                    path: "/B".into(),
                    mode: "folder"
                }
            ]
        );

        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Ok(json!({"files": ["/A/a1.mkv", "/A/a2.mkv"]})),
        );
        assert_eq!(app.current_screen, Screen::FileExplorer);
        assert!(remote(&app).pending.is_empty());
        assert!(app.queue.jobs.iter().all(|job| !job.path.starts_with("/A")));

        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Ok(json!({"files": ["/B/b1.mkv", "/B/b2.mkv"]})),
        );
        assert_eq!(app.current_screen, Screen::FileConfirm);
        let batch: Vec<PathBuf> = app.queue.jobs[app.batch_start..]
            .iter()
            .map(|job| job.path.clone())
            .collect();
        assert_eq!(
            batch,
            vec![PathBuf::from("/B/b1.mkv"), PathBuf::from("/B/b2.mkv")]
        );

        app.confirm_queued_files();
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::AddFolder {
                path: "/B".into(),
                mode: "folder",
            }]
        );
    }

    #[test]
    fn a_snapshot_keeps_the_pending_file_batch() {
        let (mut app, _sent, events) = previewed_folder();

        feed(
            &mut app,
            &events,
            snapshot_with(900, false, &["/n/only.mkv"]),
        );

        assert_eq!(app.current_screen, Screen::FileConfirm);
        assert_eq!(app.batch_start, 1);
        let batch: Vec<PathBuf> = app.queue.jobs[app.batch_start..]
            .iter()
            .map(|job| job.path.clone())
            .collect();
        assert_eq!(
            batch,
            vec![
                PathBuf::from("/srv/media/a.mkv"),
                PathBuf::from("/srv/media/b.mkv")
            ]
        );
    }

    #[test]
    fn confirming_a_folder_batch_adds_the_folder() {
        let (mut app, sent, _events) = previewed_folder();

        app.confirm_queued_files();

        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::AddFolder {
                path: "/srv/media".into(),
                mode: "folder",
            }]
        );
        assert_eq!(remote(&app).pending.len(), 0);
        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.current_screen, Screen::Queue);
        assert!(app.analysis_receiver.is_none());
    }

    /// An attached app on File Confirm with the file batch `/m/a.mkv`, `/m/b.mkv`.
    fn picked_files() -> (App, Receiver<ClientRequest>, mpsc::Sender<ClientEvent>) {
        let (mut app, sent, events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::File;
        app.dir_entries = vec![Entry {
            path: PathBuf::from("/m/b.mkv"),
            is_dir: false,
            size: None,
        }];
        app.explorer_index = 0;
        app.selected_files = vec![PathBuf::from("/m/a.mkv")];
        app.select_explorer_entry();
        (app, sent, events)
    }

    #[test]
    fn escaping_file_confirm_queues_nothing() {
        let (mut app, sent, _events) = picked_files();
        assert_eq!(app.current_screen, Screen::FileConfirm);
        assert_eq!(remote(&app).pending.len(), 2);

        app.cancel_file_confirm();

        assert_eq!(sent_requests(&sent), vec![]);
        assert_eq!(remote(&app).pending.len(), 0);
        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.current_screen, Screen::FileExplorer);
        assert_eq!(
            app.selected_files,
            vec![PathBuf::from("/m/a.mkv"), PathBuf::from("/m/b.mkv")]
        );
    }

    #[test]
    fn confirming_a_file_batch_adds_each_file() {
        let (mut app, sent, _events) = picked_files();

        app.confirm_queued_files();

        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::Add {
                paths: vec!["/m/a.mkv".into(), "/m/b.mkv".into()],
            }]
        );
        assert_eq!(remote(&app).pending.len(), 0);
        assert_eq!(app.queue.jobs.len(), 2);
        assert!(app.analysis_receiver.is_none());
    }

    #[test]
    fn a_single_file_pick_is_added_at_once() {
        let (mut app, sent, _events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::File;
        app.dir_entries = vec![Entry {
            path: PathBuf::from("/m/one.mkv"),
            is_dir: false,
            size: None,
        }];

        app.select_explorer_entry();

        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::Add {
                paths: vec!["/m/one.mkv".into()],
            }]
        );
        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.current_screen, Screen::Queue);
    }

    #[test]
    fn cancelling_a_folder_scan_while_attached_sends_nothing() {
        let (mut app, sent, _events) = attached_app();

        app.cancel_folder_scan();

        assert_eq!(sent_requests(&sent), vec![]);
    }

    /// An attached app on Track Config for job 7, with job 9 also awaiting tracks.
    fn configuring_tracks() -> (App, Receiver<ClientRequest>, mpsc::Sender<ClientEvent>) {
        let (mut app, sent, events) = attached_with_ids();
        for job in &mut app.queue.jobs {
            job.status = JobStatus::AwaitingConfig;
        }
        app.queue.jobs[0].track_selection = crate::tracks::TrackSelection {
            audio_indices: vec![1, 2],
            subtitle_indices: vec![4],
            audio_to_opus: vec![2],
        };
        app.queue.config_job_index = 0;
        app.current_screen = Screen::TrackConfig;
        (app, sent, events)
    }

    #[test]
    fn confirming_tracks_posts_the_selection() {
        let (mut app, sent, events) = configuring_tracks();

        app.confirm_track_config();
        app.confirm_track_config();

        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::SetTracks(json!({
                "id": 7,
                "audio_indices": [1, 2],
                "audio_to_opus": [2],
                "subtitle_indices": [4],
                "remux_only": false,
                "dv_mode": null,
                "apply_to_remaining": false,
            }))]
        );
        assert_eq!(app.queue.config_job_index, 0);
        assert!(!app.encoding_active);
        assert!(app.progress_receiver.is_none());

        reply(
            &mut app,
            &events,
            RequestKind::SetTracks,
            Ok(json!({"applied": 1})),
        );

        assert_eq!(app.current_screen, Screen::TrackConfig);
        assert_eq!(app.queue.config_job_index, 1);
        assert!(matches!(app.queue.jobs[0].status, JobStatus::Ready));
    }

    #[test]
    fn refused_tracks_return_to_the_queue_with_the_error() {
        let (mut app, sent, events) = configuring_tracks();
        app.confirm_track_config();
        let _ = sent_requests(&sent);

        reply(
            &mut app,
            &events,
            RequestKind::SetTracks,
            http_error("job is not editable"),
        );

        assert_eq!(app.current_screen, Screen::Queue);
        assert_eq!(app.message.as_deref(), Some("job is not editable"));
        assert_eq!(app.message_kind, MessageKind::Error);

        app.queue.config_job_index = 0;
        app.current_screen = Screen::TrackConfig;
        app.confirm_track_config();
        assert_eq!(sent_requests(&sent).len(), 1);
    }

    #[test]
    fn applying_tracks_to_the_rest_while_attached_lets_the_daemon_apply_them() {
        let (mut app, sent, events) = configuring_tracks();

        app.apply_track_config_to_remaining();

        let requests = sent_requests(&sent);
        assert_eq!(requests.len(), 1);
        let ClientRequest::SetTracks(body) = &requests[0] else {
            panic!("expected SetTracks, got {requests:?}");
        };
        assert_eq!(body["id"], json!(7));
        assert_eq!(body["apply_to_remaining"], json!(true));
        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::AwaitingConfig
        ));

        reply(
            &mut app,
            &events,
            RequestKind::SetTracks,
            Ok(json!({"applied": 2})),
        );

        assert_eq!(app.current_screen, Screen::Queue);
        assert_eq!(
            app.message.as_deref(),
            Some(
                crate::i18n::t(app.config.language, crate::i18n::Msg::WebTracksApplied)
                    .replace("{n}", "2")
                    .as_str()
            )
        );
    }

    #[test]
    fn abandoning_track_config_while_attached_keeps_the_daemon_jobs_and_rips() {
        let root =
            std::env::temp_dir().join(format!("av1c-attached-abandon-{}", std::process::id()));
        let file = crate::disc::staging::staged_rip(&root, u32::MAX - 1);
        let (mut app, sent, _events) = configuring_tracks();
        app.config.disc.staging_directory = Some(root.to_string_lossy().into_owned());
        app.queue.jobs[0].path.clone_from(&file);
        app.queue.jobs[0].temporary = true;

        app.cancel_track_config();

        assert!(file.exists());
        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.current_screen, Screen::Queue);
        assert_eq!(sent_requests(&sent), vec![]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn start_encoding_while_attached_sends_nothing_and_starts_no_worker() {
        let (mut app, sent, _events) = attached_with_ids();
        let job = &mut app.queue.jobs[0];
        job.status = JobStatus::Ready;
        job.output_path = Some(PathBuf::from("/nonexistent/out.mkv"));
        job.metadata = Some(crate::analyzer::VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: crate::analyzer::HdrType::Sdr,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "h264".into(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 1.0,
        });
        job.source_identity = Some(crate::queue::SourceIdentity::from_metadata(
            &std::fs::metadata(std::env::current_exe().unwrap()).unwrap(),
        ));

        app.start_encoding();

        assert_eq!(sent_requests(&sent), vec![]);
        assert!(app.progress_receiver.is_none());
        assert!(app.tui_lock.is_none());
        assert!(!app.encoding_active);
        assert!(matches!(app.queue.jobs[0].status, JobStatus::Ready));
    }

    #[test]
    fn disc_actions_while_attached_go_to_the_daemon() {
        let (mut app, sent, events) = attached_app();

        app.start_disc_flow();
        assert_eq!(sent_requests(&sent), vec![ClientRequest::DiscsList]);
        assert_eq!(app.current_screen, Screen::DiscDrives);
        assert_eq!(app.disc_state, DiscState::Discovering);

        reply(
            &mut app,
            &events,
            RequestKind::DiscsList,
            Ok(json!({"drives": [{"id": 3, "name": "BD-RE", "disc_label": "MOVIE"}]})),
        );
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::DiscsScan(json!({"drive": 3}))]
        );
        assert_eq!(app.current_screen, Screen::DiscTitles);

        app.scan_disc_folder(Path::new("/srv/discs/MOVIE"));
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::DiscsScan(
                json!({"folder": "/srv/discs/MOVIE"})
            )]
        );

        app.disc_state = DiscState::Ready;
        app.disc_source = Some(DiscSource::Drive(crate::disc::DiscDrive {
            id: 3,
            name: "BD-RE".into(),
            disc_label: Some("MOVIE".into()),
        }));
        app.disc_selected = vec![2, 0];
        app.start_disc_rip();
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::DiscsRip(
                json!({"drive": 3, "titles": [2, 0]})
            )]
        );
        assert_eq!(app.current_screen, Screen::Queue);
        assert_eq!(app.queue.jobs.len(), 2);
        assert!(app.disc_receiver.is_none());

        let mut ripping = snapshot_with(901, false, &[]);
        ripping.disc.active = true;
        feed(&mut app, &events, ripping);
        app.cancel_disc_operation();
        assert_eq!(sent_requests(&sent), vec![ClientRequest::DiscsCancel]);
    }

    /// The `DiscsRip` body an attached app sends for `source` with titles `[3]`.
    fn rip_body(source: DiscSource) -> Option<Value> {
        let (mut app, sent, _events) = attached_app();
        app.disc_state = DiscState::Ready;
        app.disc_source = Some(source);
        app.disc_selected = vec![3];
        app.start_disc_rip();
        match sent_requests(&sent).as_slice() {
            [ClientRequest::DiscsRip(body)] => Some(body.clone()),
            [] => None,
            other => panic!("unexpected requests {other:?}"),
        }
    }

    #[test]
    fn the_rip_body_names_the_scanned_source_the_daemon_checks() {
        let drive = DiscDrive {
            id: 0,
            name: "BD-RE".into(),
            disc_label: None,
        };
        let folder = std::env::temp_dir().canonicalize().unwrap();
        for source in [DiscSource::Drive(drive), DiscSource::Folder(folder)] {
            let body = rip_body(source.clone()).expect("a rip request");
            let mut state = crate::daemon::state::DaemonState::new(AppConfig::default());
            state.disc.scanned_source = Some(source);
            state.disc.titles = vec![DiscTitle {
                id: 3,
                name: "Feature".into(),
                duration: std::time::Duration::ZERO,
                size_bytes: 0,
                chapters: 0,
                tracks: Vec::new(),
            }];
            let shared = Arc::new(std::sync::Mutex::new(state));
            let (tx, _rx) = mpsc::channel();

            let (status, reply) = crate::daemon::api::discs_rip(&shared, &tx, &body);

            // No output directory: refused after the source and title checks pass.
            assert_eq!(
                (status, &reply["error"]),
                (
                    400,
                    &json!(DiscError::NoDestination.message(crate::i18n::Language::English))
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn a_rip_without_a_scanned_source_sends_nothing() {
        let (mut app, sent, _events) = attached_app();
        app.disc_state = DiscState::Ready;
        app.disc_selected = vec![3];

        app.start_disc_rip();

        assert_eq!(sent_requests(&sent), vec![]);
    }

    #[test]
    fn a_snapshot_carries_the_session_clock() {
        let (mut app, _sent, events) = attached_app();
        let mut idle = snapshot_with(901, false, &["/n/a.mkv"]);
        idle.elapsed_secs = Some(90);
        feed(&mut app, &events, idle);
        assert_eq!(
            app.queue.elapsed_time(),
            Some(std::time::Duration::from_secs(90))
        );

        let mut encoding = snapshot_with(902, true, &["/n/a.mkv"]);
        encoding.elapsed_secs = Some(90);
        feed(&mut app, &events, encoding);
        assert!(app.queue.elapsed_time().unwrap() >= std::time::Duration::from_secs(90));
        assert!(app.queue.end_time.is_none());

        feed(&mut app, &events, snapshot_with(903, false, &["/n/a.mkv"]));
        assert_eq!(app.queue.elapsed_time(), None);
    }

    #[test]
    fn a_second_folder_pick_waits_for_the_first_preview() {
        let (mut app, sent, _events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::Folder;
        app.scan_folder(PathBuf::from("/srv/a"), false);
        let _ = sent_requests(&sent);

        app.scan_folder(PathBuf::from("/srv/b"), false);

        assert_eq!(sent_requests(&sent), vec![]);
        assert!(app.folder_scan_active());
    }

    #[test]
    fn a_cancelled_preview_reply_is_ignored() {
        let (mut app, sent, events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::Folder;
        app.scan_folder(PathBuf::from("/srv/a"), false);
        app.cancel_folder_scan();
        assert!(!app.folder_scan_active());
        app.scan_folder(PathBuf::from("/srv/b"), false);
        let _ = sent_requests(&sent);

        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Ok(json!({"files": ["/srv/a/1.mkv", "/srv/a/2.mkv"]})),
        );
        assert_eq!(app.current_screen, Screen::FileExplorer);
        assert_eq!(app.queue.jobs.len(), 2);
        assert!(app.folder_scan_active());

        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Ok(json!({"files": ["/srv/b/1.mkv", "/srv/b/2.mkv"]})),
        );
        assert_eq!(app.current_screen, Screen::FileConfirm);
        assert_eq!(app.queue.jobs[2].path, PathBuf::from("/srv/b/1.mkv"));
        app.confirm_queued_files();
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::AddFolder {
                path: "/srv/b".into(),
                mode: "folder",
            }]
        );
    }

    #[test]
    fn a_tracks_reply_marks_the_confirmed_job_after_stepping_away() {
        let (mut app, sent, events) = configuring_tracks();
        app.confirm_track_config();
        let _ = sent_requests(&sent);
        app.step_track_config_job(true);
        assert_eq!(app.queue.config_job_index, 1);

        reply(
            &mut app,
            &events,
            RequestKind::SetTracks,
            Ok(json!({"applied": 1})),
        );

        assert!(matches!(app.queue.jobs[0].status, JobStatus::Ready));
        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::AwaitingConfig
        ));
        assert_eq!(app.queue.config_job_index, 1);
        assert_eq!(app.current_screen, Screen::TrackConfig);
    }

    #[test]
    fn reconnecting_asks_again_whether_host_settings_are_editable() {
        let (mut app, sent, events) = attached_app();
        events
            .send(ClientEvent::Unreachable("refused".into()))
            .unwrap();
        events.send(ClientEvent::Reachable).unwrap();

        app.process_client_events();

        assert_eq!(sent_requests(&sent), vec![ClientRequest::SettingsAccess]);
    }

    #[test]
    fn the_explorer_keeps_its_rows_until_the_daemon_lists_the_folder() {
        let (mut app, sent, _events) = attached_app();
        let row = Entry {
            path: PathBuf::from("/srv/old.mkv"),
            is_dir: false,
            size: None,
        };
        app.dir_entries = vec![row.clone()];
        app.current_dir = PathBuf::from("/srv/new");

        app.refresh_dir_entries();

        assert_eq!(app.dir_entries, vec![row]);
        assert_eq!(sent_requests(&sent).len(), 1);
    }

    #[test]
    fn leaving_the_disc_screen_while_unreachable_sends_nothing() {
        let (mut app, sent, events) = attached_app();
        let mut listing = snapshot_with(901, false, &[]);
        listing.disc.active = true;
        listing.disc.listing = true;
        feed(&mut app, &events, listing);
        app.current_screen = Screen::DiscDrives;
        events
            .send(ClientEvent::Unreachable("refused".into()))
            .unwrap();
        app.process_client_events();

        app.leave_disc_screen();

        assert_eq!(sent_requests(&sent), vec![]);
        assert_eq!(app.current_screen, Screen::Home);
    }

    #[test]
    fn opening_settings_while_attached_loads_the_daemon_config() {
        let (mut app, sent, events) = attached_app();

        app.navigate_to_configuration();
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::SettingsAccess, ClientRequest::SettingsGet]
        );
        assert_eq!(app.current_screen, Screen::Configuration);

        let mut daemon_config = AppConfig::default();
        daemon_config.output.suffix = "_daemon".into();
        daemon_config.daemon.port = 9123;
        let mut body = serde_json::to_value(&daemon_config).unwrap();
        body["daemon"]["auth_token"] = json!("");
        reply(
            &mut app,
            &events,
            RequestKind::SettingsGet,
            Ok(body.clone()),
        );

        assert_eq!(app.config.output.suffix, "_daemon");
        assert_eq!(app.config.daemon.port, 9123);
        assert!(!app.config_is_dirty());

        app.config.output.suffix = "_mine".into();
        reply(&mut app, &events, RequestKind::SettingsGet, Ok(body));
        assert_eq!(app.config.output.suffix, "_mine");
        assert!(app.config_is_dirty());
    }

    #[test]
    fn a_remote_attach_marks_host_settings_read_only() {
        let (mut app, _sent, events) = attached_app();

        reply(
            &mut app,
            &events,
            RequestKind::SettingsAccess,
            Ok(json!({"local": false})),
        );
        assert!(app.host_settings_read_only());

        reply(
            &mut app,
            &events,
            RequestKind::SettingsAccess,
            Ok(json!({"local": true})),
        );
        assert!(!app.host_settings_read_only());
        assert!(!App::new().host_settings_read_only());
    }

    #[test]
    fn attaching_to_an_idle_daemon_opens_on_home() {
        let (session, _sent, _events) = session();
        let (first, _) = daemon_snapshot(false);

        let app = App::attached(AppConfig::default(), session, &first);

        assert_eq!(app.current_screen, Screen::Home);
        assert!(!app.encoding_active);
        assert_eq!(app.queue.jobs.len(), 2);
        let Backend::Remote(remote) = &app.backend else {
            panic!("not attached");
        };
        assert!(!remote.was_encoding);
        assert!(remote.reachable);
    }

    #[test]
    fn an_ended_encode_session_releases_the_tui_lock() {
        let path = std::env::temp_dir().join(format!("av1c_app_tui_lock_{}", std::process::id()));
        let mut app = App::new();
        app.tui_lock = Some(crate::daemon::lifecycle::write_tui_lock_at(&path).unwrap());
        app.encoding_active = true;
        let (tx, rx) = mpsc::channel();
        app.progress_receiver = Some(rx);
        tx.send(WorkerMessage::Finished).unwrap();

        app.process_progress_messages();

        assert!(!app.encoding_active);
        assert!(app.tui_lock.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn empty_encode_session_does_not_arm_encoding() {
        let mut app = App::new();
        let mut job = EncodingJob::new(PathBuf::from("ripped.mkv"));
        job.status = JobStatus::Ready;
        job.temporary = true;
        job.output_path = None;
        // A fully analysed job, missing only its destination.
        job.metadata = Some(crate::analyzer::VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: crate::analyzer::HdrType::Sdr,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "h264".into(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 1.0,
        });
        let exe = std::env::current_exe().unwrap();
        job.source_identity = Some(crate::queue::SourceIdentity::from_metadata(
            &std::fs::metadata(exe).unwrap(),
        ));
        app.queue.jobs.push(job);

        app.start_encoding();

        assert!(!app.encoding_active);
        assert!(app.progress_receiver.is_none());
        assert!(matches!(app.queue.jobs[0].status, JobStatus::Error { .. }));
        assert_eq!(app.queue.error_count, 1);
    }

    #[test]
    fn moving_a_ready_job_up_keeps_the_cursor_on_it() {
        let mut app = App::new();
        for name in ["first.mkv", "second.mkv"] {
            let mut job = EncodingJob::new(PathBuf::from(name));
            job.status = JobStatus::Ready;
            app.queue.jobs.push(job);
        }
        app.queue_cursor = 1;

        app.queue_move_selected_up();

        assert_eq!(app.queue_cursor, 0);
        assert_eq!(app.queue.jobs[0].filename(), "second.mkv");
    }

    #[test]
    fn a_finished_job_records_its_output_size_immediately() {
        let dir =
            std::env::temp_dir().join(format!("av1c-finish-size-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("out.mkv");
        std::fs::write(&output, b"12345").unwrap();

        let mut app = App::new();
        let mut job = EncodingJob::new(PathBuf::from("source.mkv"));
        job.status = JobStatus::Encoding { progress: 99.0 };
        job.output_path = Some(output);
        app.queue.jobs.push(job);

        app.finish_job(0, JobStatus::DoneWithVmaf { score: 95.0 });

        assert_eq!(app.queue.jobs[0].output_size, Some(5));
        assert_eq!(app.queue.converted_count, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancelling_an_encode_settles_the_remaining_ready_jobs() {
        let mut app = App::new();
        let mut active = EncodingJob::new(PathBuf::from("active.mkv"));
        active.status = JobStatus::Encoding { progress: 10.0 };
        let mut waiting = EncodingJob::new(PathBuf::from("waiting.mkv"));
        waiting.status = JobStatus::Ready;
        app.queue.jobs = vec![active, waiting];
        app.encoding_active = true;

        app.cancel_encoding();

        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::Skipped { .. }
        ));
        assert_eq!(app.queue.skipped_count, 1);
        assert_eq!(app.queue.cancelled_count, 1);
    }

    #[test]
    fn cancelling_an_encode_keeps_a_queued_rip() {
        let root = std::env::temp_dir().join(format!("av1c-cancel-encode-{}", std::process::id()));
        let file = crate::disc::staging::staged_rip(&root, u32::MAX);
        let mut app = App::new();
        app.config.disc.staging_directory = Some(root.to_string_lossy().into_owned());
        let mut active = EncodingJob::new(PathBuf::from("active.mkv"));
        active.status = JobStatus::Encoding { progress: 10.0 };
        let mut rip = EncodingJob::new(file.clone());
        rip.status = JobStatus::Ready;
        rip.temporary = true;
        app.queue.jobs = vec![active, rip];
        app.encoding_active = true;

        app.cancel_encoding();

        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::Skipped { .. }
        ));
        assert!(app.queue.jobs[1].temporary);
        assert!(file.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cancelling_the_analysis_of_a_lone_rip_keeps_it() {
        let root =
            std::env::temp_dir().join(format!("av1c-cancel-analysis-{}", std::process::id()));
        let file = crate::disc::staging::staged_rip(&root, u32::MAX);
        let mut app = App::new();
        app.config.disc.staging_directory = Some(root.to_string_lossy().into_owned());
        let mut rip = EncodingJob::new(file.clone());
        rip.status = JobStatus::Analyzing;
        rip.temporary = true;
        app.queue.jobs = vec![rip];
        app.current_screen = Screen::Queue;
        let (_tx, rx) = mpsc::channel();
        app.analysis_receiver = Some(rx);

        app.cancel_analysis();

        assert_eq!(app.queue.jobs.len(), 1);
        assert!(app.queue.jobs[0].temporary);
        assert!(file.exists());
        assert_eq!(app.current_screen, Screen::Finish);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quitting_does_not_start_the_next_ready_job() {
        let mut app = App::new();
        let mut active = EncodingJob::new(PathBuf::from("active.mkv"));
        active.status = JobStatus::Encoding { progress: 10.0 };
        let mut waiting = EncodingJob::new(PathBuf::from("waiting.mkv"));
        waiting.status = JobStatus::Ready;
        app.queue.jobs = vec![active, waiting];
        app.encoding_session_indices = vec![0];
        app.encoding_active = true;
        app.should_quit = true;
        let (tx, rx) = mpsc::channel();
        tx.send(WorkerMessage::Cancelled).unwrap();
        app.progress_receiver = Some(rx);

        app.process_progress_messages();

        assert!(!app.encoding_active);
        assert!(matches!(app.queue.jobs[1].status, JobStatus::Ready));
        assert!(app.progress_receiver.is_none());
    }

    #[test]
    fn active_disc_work_blocks_the_finish_screen() {
        let mut app = App::new();
        let (_tx, rx) = mpsc::channel();
        app.disc_receiver = Some(rx);
        app.current_screen = Screen::Queue;

        app.navigate_to_finish();

        assert_eq!(app.current_screen, Screen::Queue);
    }

    #[test]
    fn unfinished_jobs_block_the_finish_screen() {
        let mut app = App::new();
        app.current_screen = Screen::Queue;
        app.queue
            .jobs
            .push(EncodingJob::new(PathBuf::from("waiting.mkv")));

        app.navigate_to_finish();

        assert_eq!(app.current_screen, Screen::Queue);
    }

    #[test]
    fn an_empty_queue_blocks_the_finish_screen() {
        let mut app = App::new();
        app.current_screen = Screen::Queue;

        app.navigate_to_finish();

        assert_eq!(app.current_screen, Screen::Queue);
    }

    #[test]
    fn a_finished_encode_session_leaves_late_titles_in_the_queue() {
        let mut app = App::new();
        let mut done = EncodingJob::new(PathBuf::from("done.mkv"));
        done.status = JobStatus::Done;
        let mut waiting = EncodingJob::new(PathBuf::from("late-title.mkv"));
        waiting.status = JobStatus::AwaitingConfig;
        app.queue.jobs = vec![done, waiting];
        app.encoding_active = true;
        app.encoding_session_indices = vec![0];
        app.current_screen = Screen::Queue;
        let (tx, rx) = mpsc::channel();
        tx.send(WorkerMessage::Finished).unwrap();
        drop(tx);
        app.progress_receiver = Some(rx);

        app.process_progress_messages();

        assert!(!app.encoding_active);
        assert_eq!(app.current_screen, Screen::Queue);
        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::AwaitingConfig
        ));
    }

    #[test]
    fn a_finished_encode_session_leaves_track_config_open() {
        let mut app = App::new();
        let mut done = EncodingJob::new(PathBuf::from("done.mkv"));
        done.status = JobStatus::Done;
        let mut waiting = EncodingJob::new(PathBuf::from("late-title.mkv"));
        waiting.status = JobStatus::AwaitingConfig;
        app.queue.jobs = vec![done, waiting];
        app.encoding_active = true;
        app.encoding_session_indices = vec![0];
        app.queue.config_job_index = 1;
        app.queue_cursor = 1;
        app.current_screen = Screen::TrackConfig;
        let (tx, rx) = mpsc::channel();
        tx.send(WorkerMessage::Finished).unwrap();
        app.progress_receiver = Some(rx);

        app.process_progress_messages();

        assert_eq!(app.current_screen, Screen::TrackConfig);
        assert_eq!(app.queue_cursor, 1);
    }

    #[test]
    fn a_finished_encode_session_leaves_file_confirm_open_and_its_batch_analysable() {
        let mut app = App::new();
        let mut done = EncodingJob::new(PathBuf::from("done.mkv"));
        done.status = JobStatus::Done;
        let new_file = EncodingJob::new(PathBuf::from("new.mkv"));
        app.queue.jobs = vec![done, new_file];
        app.batch_start = 1;
        app.encoding_active = true;
        app.encoding_session_indices = vec![0];
        app.navigate_to_file_confirm();
        let (tx, rx) = mpsc::channel();
        tx.send(WorkerMessage::Finished).unwrap();
        drop(tx);
        app.progress_receiver = Some(rx);

        app.process_progress_messages();

        assert_eq!(app.current_screen, Screen::FileConfirm);
        assert!(matches!(app.queue.jobs[1].status, JobStatus::Pending));

        app.confirm_queued_files();

        assert_eq!(app.current_screen, Screen::Queue);
        assert!(matches!(app.queue.jobs[1].status, JobStatus::Analyzing));
        assert!(app.analysis_receiver.is_some());
    }

    #[test]
    fn a_finished_encode_session_leaves_the_settings_editor_open() {
        let mut app = App::new();
        let mut done = EncodingJob::new(PathBuf::from("done.mkv"));
        done.status = JobStatus::Done;
        app.queue.jobs = vec![done];
        app.encoding_active = true;
        app.encoding_session_indices = vec![0];
        app.navigate_to_configuration();
        app.config_edit_buffer = Some("edit".to_string());
        let (tx, rx) = mpsc::channel();
        tx.send(WorkerMessage::Finished).unwrap();
        drop(tx);
        app.progress_receiver = Some(rx);

        app.process_progress_messages();

        assert_eq!(app.current_screen, Screen::Configuration);
        assert_eq!(app.config_edit_buffer.as_deref(), Some("edit"));
    }

    #[test]
    fn a_finished_analysis_round_leaves_other_screens_alone() {
        for (screen, next) in [
            (Screen::FileConfirm, JobStatus::AwaitingConfig),
            (Screen::Configuration, JobStatus::AwaitingConfig),
            (Screen::Home, JobStatus::Done),
            (Screen::FileExplorer, JobStatus::Done),
        ] {
            let mut app = App::new();
            let mut job = EncodingJob::new(PathBuf::from("a.mkv"));
            job.status = next.clone();
            app.queue.jobs = vec![job];
            app.current_screen = screen;
            let (_tx, rx) = mpsc::channel();
            app.analysis_receiver = Some(rx);

            app.finish_analysis_round();

            assert_eq!(app.current_screen, screen, "{next:?}");
        }

        // The queue still moves on to Track Config and Finish.
        for (next, expected) in [
            (JobStatus::AwaitingConfig, Screen::TrackConfig),
            (JobStatus::Done, Screen::Finish),
        ] {
            let mut app = App::new();
            let mut job = EncodingJob::new(PathBuf::from("a.mkv"));
            job.status = next;
            app.queue.jobs = vec![job];
            app.current_screen = Screen::Queue;
            let (_tx, rx) = mpsc::channel();
            app.analysis_receiver = Some(rx);

            app.finish_analysis_round();

            assert_eq!(app.current_screen, expected);
        }
    }

    #[test]
    fn a_finished_rip_run_leaves_other_screens_alone() {
        for (screen, expected) in [
            (Screen::Configuration, Screen::Configuration),
            (Screen::FileConfirm, Screen::FileConfirm),
            (Screen::Finish, Screen::Queue),
        ] {
            let mut app = App::new();
            app.current_screen = screen;
            let (tx, rx) = mpsc::channel();
            tx.send(DiscEvent::Finished).unwrap();
            app.disc_receiver = Some(rx);

            app.process_disc_events();

            assert_eq!(app.current_screen, expected);
        }
    }

    #[test]
    fn cancelling_analysis_preserves_a_concurrent_encode() {
        let mut app = App::new();
        let mut encoding = EncodingJob::new(PathBuf::from("encoding.mkv"));
        encoding.status = JobStatus::Encoding { progress: 25.0 };
        let mut analyzing = EncodingJob::new(PathBuf::from("analyzing.mkv"));
        analyzing.status = JobStatus::Analyzing;
        app.queue.jobs = vec![encoding, analyzing];
        app.encoding_active = true;
        app.current_screen = Screen::Queue;
        let (_tx, rx) = mpsc::channel();
        app.analysis_receiver = Some(rx);

        app.cancel_analysis();

        assert!(app.encoding_active);
        assert!(matches!(
            app.queue.jobs[0].status,
            JobStatus::Encoding { .. }
        ));
        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::Skipped { .. }
        ));
        assert_eq!(app.current_screen, Screen::Queue);
    }

    #[test]
    fn cancelling_analysis_keeps_analysed_jobs() {
        let mut app = App::new();
        let mut analysed = EncodingJob::new(PathBuf::from("/staging/rip-a/DISC_t00.mkv"));
        analysed.status = JobStatus::AwaitingConfig;
        analysed.temporary = true;
        let mut analyzing = EncodingJob::new(PathBuf::from("analyzing.mkv"));
        analyzing.status = JobStatus::Analyzing;
        app.queue.jobs = vec![analysed, analyzing];
        app.current_screen = Screen::Queue;
        let (_tx, rx) = mpsc::channel();
        app.analysis_receiver = Some(rx);

        app.cancel_analysis();

        assert_eq!(app.queue.jobs.len(), 2);
        assert!(matches!(
            app.queue.jobs[0].status,
            JobStatus::AwaitingConfig
        ));
        assert!(matches!(
            app.queue.jobs[1].status,
            JobStatus::Skipped { .. }
        ));
        assert_eq!(app.current_screen, Screen::TrackConfig);

        // A second confirmation after the round ended changes nothing.
        app.cancel_analysis();
        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.current_screen, Screen::TrackConfig);
    }

    #[test]
    fn analyzing_a_ripped_title_starts_with_a_fresh_analysis_token() {
        let mut app = App::new();
        let mut job = EncodingJob::new(std::path::PathBuf::from("/staging/rip-a/DISC_t00.mkv"));
        job.status = JobStatus::Ripping { progress: 0.0 };
        job.temporary = true;
        app.queue.jobs.push(job);
        app.analysis_cancel_flag.store(true, Ordering::Release);

        app.analyze_indices(&[0]);

        assert!(matches!(app.queue.jobs[0].status, JobStatus::Analyzing));
        assert_eq!(app.analysis_outstanding, 1);
        assert!(!app.analysis_cancel_flag.load(Ordering::Acquire));
    }

    #[test]
    fn a_batch_joining_a_running_round_shares_its_cancel_flag() {
        let mut app = App::new();
        let mut job = EncodingJob::new(PathBuf::from("/staging/rip-b/DISC_t01.mkv"));
        job.status = JobStatus::Pending;
        app.queue.jobs.push(job);
        let (tx, rx) = mpsc::channel();
        app.analysis_receiver = Some(rx);
        app.analysis_sender = Some(tx);
        app.analysis_outstanding = 1;
        let running = app.analysis_cancel_flag.clone();

        app.analyze_indices(&[0]);

        assert!(Arc::ptr_eq(&running, &app.analysis_cancel_flag));
    }

    #[test]
    fn a_scan_finishing_during_cancellation_returns_to_the_drives() {
        let mut app = App::new();
        app.current_screen = Screen::DiscTitles;
        app.disc_state = DiscState::Cancelling;
        let (tx, rx) = mpsc::channel();
        tx.send(DiscEvent::TitlesFound(crate::disc::DiscScan {
            disc_type: None,
            titles: Vec::new(),
        }))
        .unwrap();
        app.disc_receiver = Some(rx);

        app.process_disc_events();

        assert_eq!(app.current_screen, Screen::DiscDrives);
        assert_eq!(app.disc_state, DiscState::Ready);
        assert!(app.disc_receiver.is_none());
    }

    #[test]
    fn a_disc_error_while_cancelling_settles_the_ripping_jobs() {
        let mut app = App::new();
        let mut job = EncodingJob::new(PathBuf::from("/staging/rip-a/DISC_t00.mkv"));
        job.status = JobStatus::Ripping { progress: 10.0 };
        job.temporary = true;
        app.queue.jobs.push(job);
        app.disc_job_indices = vec![0];
        app.disc_state = DiscState::Cancelling;
        app.current_screen = Screen::Queue;
        let (tx, rx) = mpsc::channel();
        tx.send(DiscEvent::Error {
            index: 0,
            error: crate::disc::DiscError::Failed("drive gone".to_string()),
        })
        .unwrap();
        app.disc_receiver = Some(rx);

        app.process_disc_events();

        assert!(app.queue.jobs[0].status.is_terminal());
        assert!(app.queue.all_completed());
        assert_eq!(app.disc_state, DiscState::Ready);
    }

    #[test]
    fn cancelling_track_config_drops_the_analysis_round() {
        let mut app = App::new();
        let (tx, rx) = mpsc::channel();
        app.analysis_receiver = Some(rx);
        app.analysis_sender = Some(tx);
        app.analysis_outstanding = 2;

        app.cancel_track_config();

        assert!(app.analysis_receiver.is_none());
        assert!(app.analysis_sender.is_none());
        assert_eq!(app.analysis_outstanding, 0);
    }

    #[test]
    fn reset_discards_staged_rips() {
        let root = std::env::temp_dir().join(format!("av1c-reset-staging-{}", std::process::id()));
        let file = crate::disc::staging::staged_rip(&root, u32::MAX);
        let dir = file.parent().unwrap().to_path_buf();

        let mut app = App::new();
        app.config.disc.staging_directory = Some(root.to_string_lossy().into_owned());
        let mut job = EncodingJob::new(file);
        job.status = JobStatus::Ready;
        job.temporary = true;
        app.queue.jobs.push(job);

        app.reset();

        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_the_job_being_encoded_is_closed_to_track_changes() {
        let mut app = App::new();
        let mut awaiting = EncodingJob::new(PathBuf::from("a.mkv"));
        awaiting.status = JobStatus::AwaitingConfig;
        let mut ready = EncodingJob::new(PathBuf::from("b.mkv"));
        ready.status = JobStatus::Ready;
        let mut encoding = EncodingJob::new(PathBuf::from("c.mkv"));
        encoding.status = JobStatus::Ready;
        app.queue.jobs = vec![awaiting, ready, encoding];
        app.encoding_active = true;
        app.encoding_session_indices = vec![2];
        assert!(app.is_track_configurable(0));
        assert!(app.is_track_configurable(1));
        assert!(!app.is_track_configurable(2));
    }

    #[test]
    fn confirming_the_first_job_starts_encoding_while_others_await() {
        let mut app = App::new();
        let metadata = crate::analyzer::VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: crate::analyzer::HdrType::Sdr,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "h264".to_string(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 1.0,
        };
        let dir = std::env::temp_dir().join(format!("av1c-first-confirm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        app.queue.jobs = ["a.mkv", "b.mkv"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                std::fs::write(&path, b"video").unwrap();
                let mut job = EncodingJob::new(path.clone());
                job.status = JobStatus::AwaitingConfig;
                job.metadata = Some(metadata.clone());
                job.source_identity =
                    crate::queue::SourceIdentity::from_path(path.to_str().unwrap()).ok();
                job.output_path = Some(dir.join(format!("out-{name}")));
                job
            })
            .collect();
        app.current_screen = Screen::TrackConfig;
        app.queue.config_job_index = 0;

        app.confirm_track_config();

        assert!(app.encoding_active);
        assert_eq!(app.encoding_session_indices, vec![0]);
        assert_eq!(app.queue.config_job_index, 1);
        assert_eq!(app.current_screen, Screen::TrackConfig);
        // The worker holds the sender; draining until it disconnects means the
        // thread has ended and nothing is still reading the directory.
        app.cancel_flag.store(true, Ordering::Release);
        let rx = app.progress_receiver.take().expect("a worker receiver");
        while rx.recv().is_ok() {}
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn applying_to_remaining_configures_every_awaiting_job() {
        let mut app = App::new();
        let track = |index| crate::tracks::AudioTrack {
            index,
            language: None,
            codec: "ac3".to_string(),
            channels: Some(2),
            channel_layout: None,
            title: None,
            bitrate: None,
            sample_rate: None,
        };
        app.queue.jobs = ["a.mkv", "b.mkv", "c.mkv"]
            .iter()
            .map(|name| {
                let mut job = EncodingJob::new(PathBuf::from(name));
                job.status = JobStatus::AwaitingConfig;
                job.audio_tracks = vec![track(0), track(1)];
                job.track_selection.audio_indices = vec![0];
                job
            })
            .collect();
        app.queue.jobs[0].track_selection.audio_indices = vec![1];
        app.queue.jobs[0].track_selection.audio_to_opus = vec![1];
        app.current_screen = Screen::TrackConfig;

        app.apply_track_config_to_remaining();

        for job in &app.queue.jobs {
            assert!(matches!(job.status, JobStatus::Ready));
            assert_eq!(job.track_selection.audio_indices, vec![1]);
            assert_eq!(job.track_selection.audio_to_opus, vec![1]);
        }
    }

    #[test]
    fn removing_a_waiting_job_behind_the_encode_keeps_the_encode_index() {
        let mut app = App::new();
        app.queue.jobs = ["a.mkv", "b.mkv", "c.mkv"]
            .iter()
            .map(|name| {
                let mut job = EncodingJob::new(PathBuf::from(name));
                job.status = JobStatus::Ready;
                job
            })
            .collect();
        app.queue.jobs[0].status = JobStatus::Encoding { progress: 10.0 };
        app.queue.current_job_index = 0;
        app.encoding_active = true;
        app.queue.total_jobs_to_encode = 3;

        assert!(!app.can_remove_job(0));
        app.remove_job(2);

        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.queue.current_job_index, 0);
        assert_eq!(app.queue.total_jobs_to_encode, 2);
    }

    #[test]
    fn clearing_finished_keeps_waiting_jobs_and_the_saved_total() {
        let mut app = App::new();
        let mut done = EncodingJob::new(PathBuf::from("done.mkv"));
        done.status = JobStatus::Done;
        done.source_size = Some(1000);
        done.output_size = Some(400);
        let mut waiting = EncodingJob::new(PathBuf::from("waiting.mkv"));
        waiting.status = JobStatus::AwaitingConfig;
        app.queue.jobs = vec![done, waiting];
        let saved = app.queue.total_space_saved().0;

        assert_eq!(app.clear_finished(), 1);

        assert_eq!(app.queue.jobs.len(), 1);
        assert_eq!(app.queue.total_space_saved().0, saved);
    }

    #[test]
    fn a_selects_every_title_then_clears_them() {
        let mut app = App::new();
        app.disc_titles = (0..3)
            .map(|id| crate::disc::DiscTitle {
                id,
                name: format!("Title {id}"),
                duration: std::time::Duration::from_mins(20),
                size_bytes: 0,
                chapters: 0,
                tracks: Vec::new(),
            })
            .collect();
        app.disc_selected = vec![1];
        app.toggle_all_disc_titles();
        assert_eq!(app.disc_selected, vec![0, 1, 2]);
        app.toggle_all_disc_titles();
        assert_eq!(app.disc_selected, Vec::<u32>::new());
    }

    #[test]
    fn opening_files_appends_and_keeps_a_queued_rip() {
        let root = std::env::temp_dir().join(format!("av1c-choose-staging-{}", std::process::id()));
        let file = crate::disc::staging::staged_rip(&root, u32::MAX);
        let dir = file.parent().unwrap().to_path_buf();
        let videos =
            std::env::temp_dir().join(format!("av1c-choose-videos-{}", std::process::id()));
        std::fs::create_dir_all(&videos).unwrap();
        std::fs::write(videos.join("a.mkv"), b"video").unwrap();

        let mut app = App::new();
        app.config.disc.staging_directory = Some(root.to_string_lossy().into_owned());
        let mut job = EncodingJob::new(file);
        job.status = JobStatus::Ready;
        job.temporary = true;
        app.queue.jobs.push(job);
        app.selection_mode = SelectionMode::File;
        app.current_dir.clone_from(&videos);
        app.refresh_dir_entries();
        app.explorer_index = app
            .dir_entries
            .iter()
            .position(|entry| entry.path.ends_with("a.mkv"))
            .unwrap();

        app.select_explorer_entry();

        assert!(dir.exists(), "the queued rip stays");
        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.batch_start, 1);

        // The same file again adds nothing.
        app.select_explorer_entry();
        assert_eq!(app.queue.jobs.len(), 2);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(videos);
    }

    #[test]
    fn reset_starts_with_a_fresh_analysis_token() {
        let mut app = App::new();
        app.analysis_cancel_flag.store(true, Ordering::Release);

        app.reset();

        assert!(!app.analysis_cancel_flag.load(Ordering::Acquire));
    }

    #[test]
    fn space_off_a_subfolder_selects_the_open_folder() {
        let dir = std::env::temp_dir().join(format!("av1c-folder-select-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new();
        app.selection_mode = SelectionMode::Folder;
        app.current_dir.clone_from(&dir);
        app.refresh_dir_entries();

        app.select_explorer_entry();

        assert_eq!(app.current_dir, dir);
        assert!(app.folder_scan_receiver.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn folder_scan_errors_are_not_reported_as_empty_folders() {
        let mut app = App::new();
        let (tx, rx) = mpsc::channel();
        tx.send(Err("permission denied".to_string())).unwrap();
        app.folder_scan_receiver = Some(rx);

        app.process_folder_scan();

        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("permission denied")
        );
        assert_eq!(app.message_kind, MessageKind::Error);
        assert!(app.queue.jobs.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn explorer_lists_symlinked_directories_as_directories() {
        let root =
            std::env::temp_dir().join(format!("av1c_explorer_symlink_{}", std::process::id()));
        let target = root.join("target");
        let link = root.join("linked");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut app = App::new();
        app.current_dir = root.clone();

        app.refresh_dir_entries();

        assert!(
            app.dir_entries
                .iter()
                .any(|entry| entry.path == link && entry.is_dir)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_disconnected_cancelled_disc_worker_settles_as_cancelled() {
        let mut app = App::new();
        let (tx, rx) = mpsc::channel();
        drop(tx);
        app.disc_receiver = Some(rx);
        app.disc_state = DiscState::Cancelling;
        app.current_screen = Screen::Queue;
        let mut job = EncodingJob::new(PathBuf::from("title.mkv"));
        job.status = JobStatus::Ripping { progress: 20.0 };
        app.queue.jobs.push(job);

        app.process_disc_events();

        assert_eq!(app.disc_state, DiscState::Ready);
        assert!(app.disc_receiver.is_none());
        assert!(matches!(
            app.queue.jobs[0].status,
            JobStatus::Skipped { .. }
        ));
    }

    #[test]
    fn track_confirmation_changes_only_configurable_jobs() {
        let mut app = App::new();
        let mut active = EncodingJob::new(PathBuf::from("active.mkv"));
        active.status = JobStatus::Encoding { progress: 50.0 };
        let mut waiting = EncodingJob::new(PathBuf::from("waiting.mkv"));
        waiting.status = JobStatus::AwaitingConfig;
        app.queue.jobs = vec![active, waiting];
        app.encoding_active = true;

        app.queue.config_job_index = 0;
        app.confirm_track_config();
        assert!(matches!(
            app.queue.jobs[0].status,
            JobStatus::Encoding { .. }
        ));

        app.queue.config_job_index = 1;
        app.confirm_track_config();
        assert!(matches!(app.queue.jobs[1].status, JobStatus::Ready));
    }

    #[test]
    fn track_confirmation_wraps_around_to_earlier_unconfigured_jobs() {
        let mut app = App::new();
        app.queue.jobs = ["a.mkv", "b.mkv", "c.mkv"]
            .iter()
            .map(|name| {
                let mut job = EncodingJob::new(PathBuf::from(name));
                job.status = JobStatus::AwaitingConfig;
                job
            })
            .collect();
        app.queue.config_job_index = 2;

        app.confirm_track_config();

        assert_eq!(app.queue.config_job_index, 0);
        assert!(!app.encoding_active);
    }

    #[test]
    fn queue_enter_opens_the_highlighted_awaiting_job() {
        let mut app = App::new();
        let mut first = EncodingJob::new(PathBuf::from("first.mkv"));
        first.status = JobStatus::AwaitingConfig;
        let mut second = EncodingJob::new(PathBuf::from("second.mkv"));
        second.status = JobStatus::AwaitingConfig;
        app.queue.jobs = vec![first, second];
        app.queue_cursor = 1;

        app.configure_next_job();

        assert_eq!(app.queue.config_job_index, 1);
        assert_eq!(app.current_screen, Screen::TrackConfig);
    }

    #[test]
    fn a_late_analysis_error_does_not_overwrite_a_failed_job() {
        let mut app = App::new();
        let mut job = EncodingJob::new(PathBuf::from("gone.mkv"));
        job.status = JobStatus::Error {
            message: "probe failed".to_string(),
        };
        app.queue.jobs.push(job);
        app.queue.error_count = 1;

        app.apply_analysis_result(0, Err(AppError::Analysis("late".to_string())));

        assert!(matches!(
            &app.queue.jobs[0].status,
            JobStatus::Error { message } if message == "probe failed"
        ));
        assert_eq!(app.queue.error_count, 1);
    }

    #[test]
    fn an_analysis_names_the_output_with_the_saved_suffix() {
        let mut app = App::new();
        app.saved_config.output.suffix = "_saved".to_string();
        app.saved_config.output.same_directory = true;
        app.saved_config.output.container = "mkv".to_string();
        app.config = app.saved_config.clone();
        app.config.output.suffix = "_unsaved".to_string();
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
        job.status = JobStatus::Analyzing;
        app.queue.jobs.push(job);
        let exe = std::env::current_exe().unwrap();

        app.apply_analysis_result(
            0,
            Ok(AnalysisResult {
                metadata: crate::analyzer::VideoMetadata {
                    width: 1920,
                    height: 1080,
                    hdr_type: HdrType::Sdr,
                    dv_profile: None,
                    dv_bl_compat: None,
                    hdr10_static: None,
                    codec_name: "h264".into(),
                    frame_rate_num: 24,
                    frame_rate_den: 1,
                    duration_secs: 1.0,
                },
                audio_tracks: Vec::new(),
                subtitle_tracks: Vec::new(),
                source_identity: crate::queue::SourceIdentity::from_path(exe).unwrap(),
            }),
        );

        assert_eq!(
            app.queue.jobs[0].output_path,
            Some(PathBuf::from("/tmp/movie_saved.mkv"))
        );
    }

    /// A snapshot of one job with `status`, taken at `taken_ms`.
    fn snapshot_of_status(
        taken_ms: u64,
        encoding: bool,
        status: JobStatus,
    ) -> crate::daemon::client::RemoteSnapshot {
        let mut snapshot = snapshot_with(taken_ms, encoding, &["/n/a.mkv"]);
        snapshot.queue.state.jobs[0].status = status;
        snapshot
    }

    #[test]
    fn an_encode_ending_on_settings_or_the_explorer_keeps_the_screen() {
        for screen in [Screen::Configuration, Screen::FileExplorer] {
            let (mut app, _sent, events) = attached_app();
            feed(&mut app, &events, snapshot_with(900, true, &["/n/a.mkv"]));
            app.current_screen = screen;

            feed(
                &mut app,
                &events,
                snapshot_of_status(901, false, JobStatus::Done),
            );

            assert_eq!(app.current_screen, screen);
            assert!(!remote(&app).was_encoding);
        }
    }

    #[test]
    fn a_restarted_daemon_with_an_older_clock_is_applied() {
        let (mut app, _sent, events) = attached_app();
        feed(
            &mut app,
            &events,
            snapshot_with(90_000, false, &["/n/a.mkv"]),
        );
        let mut restarted = snapshot_with(5, false, &["/n/new.mkv", "/n/new2.mkv"]);
        restarted.instance = remote(&app).instance + 1;

        feed(&mut app, &events, restarted);

        assert_eq!(app.queue.jobs.len(), 2);
        assert_eq!(app.queue.jobs[0].path, PathBuf::from("/n/new.mkv"));
        assert_eq!(remote(&app).last_taken_ms, 5);
        assert!(!app.daemon_blocked());
    }

    #[test]
    fn a_restart_ending_the_old_encode_does_not_open_finish() {
        let (mut app, _sent, events) = attached_app();
        feed(
            &mut app,
            &events,
            snapshot_with(90_000, true, &["/n/a.mkv"]),
        );
        app.navigate_to_queue();
        let mut restarted = snapshot_of_status(5, false, JobStatus::Done);
        restarted.instance = remote(&app).instance + 1;

        feed(&mut app, &events, restarted);

        assert_eq!(app.current_screen, Screen::Queue);
        assert!(!remote(&app).was_encoding);
    }

    #[test]
    fn a_restarted_daemon_of_another_version_is_blocked_with_a_message() {
        let (mut app, _sent, events) = attached_app();
        let mut restarted = snapshot_with(5, false, &["/n/other.mkv"]);
        restarted.instance = remote(&app).instance + 1;
        restarted.version = "0.0.1-old".into();

        feed(&mut app, &events, restarted);

        assert!(app.daemon_blocked());
        assert_eq!(remote(&app).version_mismatch.as_deref(), Some("0.0.1-old"));
        assert_eq!(
            app.message.as_deref(),
            Some(
                crate::i18n::t(app.config.language, crate::i18n::Msg::DaemonVersionMismatch)
                    .replace("{daemon}", "0.0.1-old")
                    .replace("{tui}", env!("CARGO_PKG_VERSION"))
                    .as_str()
            )
        );
        assert_eq!(app.queue.jobs[0].path, PathBuf::from("/nonexistent/a.mkv"));
    }

    #[test]
    fn a_rejected_token_shows_its_own_message_until_a_snapshot_arrives() {
        let (mut app, _sent, events) = attached_app();
        events.send(ClientEvent::TokenRejected).unwrap();
        app.process_client_events();

        let rejected = crate::i18n::t(app.config.language, crate::i18n::Msg::DaemonTokenRejected);
        assert!(app.daemon_blocked());
        assert!(remote(&app).reachable);
        assert_eq!(app.message.as_deref(), Some(rejected));

        feed(&mut app, &events, snapshot_with(900, false, &["/n/a.mkv"]));

        assert!(!app.daemon_blocked());
        assert!(app.message.is_none());
    }

    #[test]
    fn a_401_reply_blocks_with_the_token_message() {
        let (mut app, _sent, events) = attached_app();
        reply(
            &mut app,
            &events,
            RequestKind::Remove,
            Err(ClientError::Http {
                status: 401,
                error: "missing or invalid token".into(),
            }),
        );

        assert!(remote(&app).token_rejected);
        assert_eq!(
            app.message.as_deref(),
            Some(crate::i18n::t(
                app.config.language,
                crate::i18n::Msg::DaemonTokenRejected
            ))
        );
    }

    #[test]
    fn an_unreachable_preview_reply_ends_the_scan_with_an_error() {
        let (mut app, _sent, events) = attached_app();
        app.current_screen = Screen::FileExplorer;
        app.selection_mode = SelectionMode::FolderRecursive;
        app.scan_folder(PathBuf::from("/srv/media"), true);
        assert!(app.folder_scan_active());

        reply(
            &mut app,
            &events,
            RequestKind::AddPreview,
            Err(ClientError::Unreachable("timed out".into())),
        );

        assert!(!app.folder_scan_active());
        assert_eq!(
            app.message.as_deref(),
            Some("The daemon did not answer: timed out")
        );
        assert_eq!(app.message_kind, MessageKind::Error);
    }

    #[test]
    fn analysis_ending_while_attached_opens_track_config() {
        let (mut app, _sent, events) = attached_app();
        app.navigate_to_queue();
        feed(
            &mut app,
            &events,
            snapshot_of_status(900, false, JobStatus::Analyzing),
        );

        feed(
            &mut app,
            &events,
            snapshot_of_status(901, false, JobStatus::AwaitingConfig),
        );

        assert_eq!(app.current_screen, Screen::TrackConfig);
        assert_eq!(app.queue.config_job_index, 0);
    }

    #[test]
    fn analysis_ending_on_settings_or_the_explorer_keeps_the_screen() {
        for screen in [Screen::Configuration, Screen::FileExplorer] {
            let (mut app, _sent, events) = attached_app();
            feed(
                &mut app,
                &events,
                snapshot_of_status(900, false, JobStatus::Analyzing),
            );
            app.current_screen = screen;

            feed(
                &mut app,
                &events,
                snapshot_of_status(901, false, JobStatus::AwaitingConfig),
            );

            assert_eq!(app.current_screen, screen);
        }
    }

    #[test]
    fn jobs_still_awaiting_config_do_not_reopen_track_config() {
        let (mut app, _sent, events) = attached_app();
        app.navigate_to_queue();
        feed(
            &mut app,
            &events,
            snapshot_of_status(900, false, JobStatus::Analyzing),
        );
        feed(
            &mut app,
            &events,
            snapshot_of_status(901, false, JobStatus::AwaitingConfig),
        );
        assert_eq!(app.current_screen, Screen::TrackConfig);
        app.navigate_to_queue();

        feed(
            &mut app,
            &events,
            snapshot_of_status(902, false, JobStatus::AwaitingConfig),
        );

        assert_eq!(app.current_screen, Screen::Queue);
    }

    #[test]
    fn an_add_that_added_nothing_says_so() {
        for kind in [RequestKind::Add, RequestKind::AddFolder] {
            let (mut app, _sent, events) = attached_app();
            app.clear_message();
            reply(
                &mut app,
                &events,
                kind,
                Ok(json!({"added": 0, "already_queued": 2})),
            );
            assert_eq!(
                app.message.as_deref(),
                Some(crate::i18n::t(
                    app.config.language,
                    crate::i18n::Msg::WebNothingAdded
                ))
            );

            let (mut app, _sent, events) = attached_app();
            app.clear_message();
            reply(
                &mut app,
                &events,
                kind,
                Ok(json!({"added": 1, "already_queued": 0})),
            );
            assert!(app.message.is_none());
        }
    }

    #[test]
    fn new_conversion_while_attached_clears_finished_and_goes_home() {
        let (mut app, sent, _events) = attached_app();
        app.queue.jobs[0].status = JobStatus::Done;
        app.current_screen = Screen::Finish;

        app.new_conversion();

        assert_eq!(app.current_screen, Screen::Home);
        assert!(app.confirm_dialog.is_none());
        assert_eq!(
            sent_requests(&sent),
            vec![ClientRequest::ClearFinished(json!({"confirm": false}))]
        );
        assert_eq!(app.queue.jobs.len(), 2);
    }

    #[test]
    fn new_conversion_while_attached_with_a_finished_rip_asks_first() {
        let (mut app, sent, _events) = attached_app();
        app.queue.jobs[0].status = JobStatus::Done;
        app.queue.jobs[0].temporary = true;
        app.current_screen = Screen::Finish;

        app.new_conversion();

        assert_eq!(app.current_screen, Screen::Home);
        assert_eq!(
            app.confirm_dialog,
            Some((ConfirmAction::ClearFinishedRips, false))
        );
        assert_eq!(sent_requests(&sent), vec![]);
        assert_eq!(app.queue.jobs.len(), 2);
    }

    /// A local app with one ripped Ready job that has no destination.
    fn app_with_undeliverable_job() -> App {
        let mut app = App::new();
        let mut job = EncodingJob::new(PathBuf::from("ripped.mkv"));
        job.status = JobStatus::Ready;
        job.temporary = true;
        job.metadata = Some(crate::analyzer::VideoMetadata {
            width: 1920,
            height: 1080,
            hdr_type: crate::analyzer::HdrType::Sdr,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "h264".into(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 1.0,
        });
        job.source_identity = Some(crate::queue::SourceIdentity::from_metadata(
            &std::fs::metadata(std::env::current_exe().unwrap()).unwrap(),
        ));
        app.queue.jobs.push(job);
        app
    }

    #[test]
    fn a_running_daemon_refuses_a_local_encode() {
        let mut app = app_with_undeliverable_job();
        app.daemon_running = || Some(4242);

        app.start_encoding();

        assert!(!app.encoding_active);
        assert!(app.progress_receiver.is_none());
        assert!(app.tui_lock.is_none());
        assert!(matches!(app.queue.jobs[0].status, JobStatus::Ready));
        assert_eq!(
            app.message.as_deref(),
            Some(crate::i18n::t(
                app.config.language,
                crate::i18n::Msg::DaemonStartedElsewhere
            ))
        );
    }

    #[test]
    fn no_running_daemon_lets_a_local_encode_go_ahead() {
        let mut app = app_with_undeliverable_job();
        app.daemon_running = || None;

        app.start_encoding();

        assert!(matches!(app.queue.jobs[0].status, JobStatus::Error { .. }));
    }
}
