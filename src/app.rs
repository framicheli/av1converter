use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use std::path::PathBuf;
use std::time::Instant;

use crate::components::file_explorer::FileExplorer;
use crate::data::{AudioMode, EncodeConfig, EncodeProgress, VideoMetadata};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CurrentScreen {
    Home,
    FileExplorer,
    Config,
    Encode,
    Finish,
}

impl Default for CurrentScreen {
    fn default() -> Self {
        CurrentScreen::Home
    }
}

/// Main Application
#[derive(Debug)]
pub struct App {
    pub current_screen: CurrentScreen,
    pub explorer: FileExplorer,
    pub current_video: Option<VideoMetadata>,
    pub encode_config: EncodeConfig,
    pub encode_progress: EncodeProgress,
    pub encode_start_time: Option<Instant>,
    pub encode_finish_time: Option<Instant>,
    pub home_selection: usize,
    pub config_selection: usize,
    running: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            current_screen: CurrentScreen::Home,
            explorer: FileExplorer::default(),
            current_video: None,
            encode_config: EncodeConfig::default(),
            encode_progress: EncodeProgress::default(),
            encode_start_time: None,
            encode_finish_time: None,
            home_selection: 0,
            config_selection: 0,
            running: true,
        }
    }
}

impl App {
    /// Construct a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the application's main loop.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.running = true;
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_crossterm_events()?;
        }
        Ok(())
    }

    /// Renders the user interface.
    fn render(&mut self, frame: &mut ratatui::Frame) {
        crate::ui::ui(frame, self);
    }

    /// Reads the crossterm events and updates the state of [`App`].
    ///
    /// If your application needs to perform work in between handling events, you can use the
    /// [`event::poll`] function to check if there are any events available with a timeout.
    fn handle_crossterm_events(&mut self) -> Result<()> {
        match event::read()? {
            // it's important to check KeyEventKind::Press to avoid handling key release events
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            _ => {}
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    fn on_key_event(&mut self, key: KeyEvent) {
        match self.current_screen {
            CurrentScreen::Home => self.handle_home_keys(key),
            CurrentScreen::FileExplorer => self.handle_file_explorer_keys(key),
            CurrentScreen::Config => self.handle_config_keys(key),
            CurrentScreen::Encode => self.handle_encode_keys(key),
            CurrentScreen::Finish => self.handle_finish_keys(key),
        }
    }

    fn handle_home_keys(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => self.quit(),
            (_, KeyCode::Up) => {
                if self.home_selection > 0 {
                    self.home_selection -= 1;
                }
            }
            (_, KeyCode::Down) => {
                if self.home_selection < 1 {
                    self.home_selection += 1;
                }
            }
            (_, KeyCode::Enter) => {
                match self.home_selection {
                    0 => {
                        // Open video file
                        self.current_screen = CurrentScreen::FileExplorer;
                    }
                    1 => {
                        // Open folder
                        self.current_screen = CurrentScreen::FileExplorer;
                        // TODO: Set folder mode
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_file_explorer_keys(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.current_screen = CurrentScreen::Home;
            }
            (_, KeyCode::Up) => {
                self.explorer.previous();
            }
            (_, KeyCode::Down) => {
                self.explorer.next();
            }
            (_, KeyCode::Enter) => {
                if let Some(path) = self.explorer.select() {
                    // Check if it's a video file
                    if path.is_file() {
                        // Load video metadata and go to config screen
                        if let Ok(metadata) = self.load_video_metadata(&path) {
                            self.current_video = Some(metadata);
                            self.current_screen = CurrentScreen::Config;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_config_keys(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.current_screen = CurrentScreen::Home;
            }
            (_, KeyCode::Up) => {
                if self.config_selection > 0 {
                    self.config_selection -= 1;
                }
            }
            (_, KeyCode::Down) => {
                if self.config_selection < 3 {
                    self.config_selection += 1;
                }
            }
            (_, KeyCode::Left | KeyCode::Right | KeyCode::Enter) => {
                // Change the selected config value
                self.toggle_config_value();
            }
            (_, KeyCode::Char('s')) => {
                // Start encoding
                self.start_encoding();
            }
            _ => {}
        }
    }

    fn toggle_config_value(&mut self) {
        match self.config_selection {
            0 => {
                // Toggle quality: Low -> Medium -> High -> Low
                use crate::data::QualityPreset;
                self.encode_config.quality = match self.encode_config.quality {
                    QualityPreset::Low => QualityPreset::Medium,
                    QualityPreset::Medium => QualityPreset::High,
                    QualityPreset::High => QualityPreset::Low,
                };
            }
            1 => {
                // Toggle no_upscaling
                self.encode_config.no_upscaling = !self.encode_config.no_upscaling;
            }
            2 => {
                // Toggle audio mode
                self.encode_config.audio_mode = match self.encode_config.audio_mode {
                    AudioMode::CopyAll => AudioMode::SelectTracks,
                    AudioMode::SelectTracks => AudioMode::CopyAll,
                };
            }
            3 => {
                // Output path - for now, just toggle between None and input directory
                if let Some(ref video) = self.current_video {
                    if self.encode_config.output_path.is_none() {
                        // Set to input directory
                        if let Some(parent) = video.filepath.parent() {
                            self.encode_config.output_path = Some(parent.to_path_buf());
                        }
                    } else {
                        // Reset to None (same as input)
                        self.encode_config.output_path = None;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_encode_keys(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                // Can't cancel during encoding
            }
            _ => {}
        }
    }

    fn handle_finish_keys(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Enter) => {
                self.current_screen = CurrentScreen::Home;
                self.current_video = None;
                self.encode_progress = EncodeProgress::default();
                self.encode_start_time = None;
                self.encode_finish_time = None;
            }
            _ => {}
        }
    }

    fn load_video_metadata(&self, path: &PathBuf) -> Result<VideoMetadata> {
        use ffmpeg_next as ffmpeg;
        use std::path::Path;

        transcoder::init::ensure().map_err(|e| anyhow::anyhow!("FFmpeg init failed: {}", e))?;
        let input = ffmpeg::format::input(Path::new(path))?;

        let container = input.format().name().to_string();
        let duration = input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64;

        // Get video stream info
        let video_stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;

        let codec_id = video_stream.parameters().id();
        let video_codec = match codec_id {
            ffmpeg::codec::Id::HEVC => transcoder::VideoCodec::HEVC,
            ffmpeg::codec::Id::H264 => transcoder::VideoCodec::H264,
            ffmpeg::codec::Id::VP9 => transcoder::VideoCodec::VP9,
            ffmpeg::codec::Id::AV1 => transcoder::VideoCodec::AV1,
            _ => transcoder::VideoCodec::Unknown,
        };

        let decoder = ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())?
            .decoder()
            .video()?;

        let resolution = transcoder::VideoResolution::new(decoder.width(), decoder.height());

        // Get audio tracks
        let mut audio_tracks = Vec::new();
        for stream in input.streams() {
            if stream.parameters().medium() == ffmpeg::media::Type::Audio {
                let decoder =
                    ffmpeg::codec::context::Context::from_parameters(stream.parameters())?
                        .decoder()
                        .audio()?;

                let codec_name = stream.parameters().id().name();
                let language = stream
                    .metadata()
                    .get("language")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                audio_tracks.push(crate::data::AudioTrack {
                    index: stream.index(),
                    language,
                    codec: codec_name.to_string(),
                    channels: decoder.channels(),
                    sample_rate: decoder.rate() as i32,
                });
            }
        }

        Ok(VideoMetadata {
            filepath: path.clone(),
            container,
            video_codec,
            resolution,
            duration,
            audio_tracks,
        })
    }

    fn start_encoding(&mut self) {
        if self.current_video.is_some() {
            self.current_screen = CurrentScreen::Encode;
            self.encode_start_time = Some(Instant::now());
            // TODO: Start actual encoding in background thread
            // Simulate encoding for now
            self.encode_progress.percentage = 100.0;
            if let Some(start) = self.encode_start_time {
                let elapsed = start.elapsed();
                self.encode_progress.elapsed_time = elapsed;
                self.encode_finish_time = Some(Instant::now());
                // Transition to finish screen
                self.current_screen = CurrentScreen::Finish;
            }
        }
    }

    /// Set running to false to quit the application.
    fn quit(&mut self) {
        self.running = false;
    }
}
