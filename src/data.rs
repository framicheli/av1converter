use crate::analysis::{AnalysisResult, Resolution};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub channels: u16,
    pub title: Option<String>,
}

impl AudioTrack {
    pub fn display_name(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let title = self
            .title
            .as_ref()
            .map(|t| format!(" - {}", t))
            .unwrap_or_default();
        let channels_str = match self.channels {
            1 => "Mono",
            2 => "Stereo",
            6 => "5.1",
            8 => "7.1",
            _ => "Multi",
        };
        format!(
            "{}: {} ({} {}){}",
            self.index,
            lang,
            self.codec.to_uppercase(),
            channels_str,
            title
        )
    }
}

#[derive(Debug, Clone)]
pub struct SubtitleTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub title: Option<String>,
}

impl SubtitleTrack {
    pub fn display_name(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let title = self
            .title
            .as_ref()
            .map(|t| format!(" - {}", t))
            .unwrap_or_default();
        format!(
            "{}: {} ({}){}",
            self.index,
            lang,
            self.codec.to_uppercase(),
            title
        )
    }
}

#[derive(Debug, Clone)]
pub enum FileStatus {
    Pending,
    Analyzing,
    AwaitingConfig,
    ReadyToConvert,
    Converting { progress: f32 },
    Done,
    Skipped { reason: String },
    Error { message: String },
}

impl FileStatus {
    #[allow(dead_code)]
    pub fn symbol(&self) -> &'static str {
        match self {
            FileStatus::Pending => "○",
            FileStatus::Analyzing => "◐",
            FileStatus::AwaitingConfig => "◑",
            FileStatus::ReadyToConvert => "●",
            FileStatus::Converting { .. } => "▶",
            FileStatus::Done => "✓",
            FileStatus::Skipped { .. } => "⊘",
            FileStatus::Error { .. } => "✗",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VideoFile {
    pub path: PathBuf,
    pub analysis: Option<AnalysisResult>,
    pub resolution: Option<Resolution>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub selected_audio: Vec<usize>,
    pub selected_subtitles: Vec<usize>,
    pub status: FileStatus,
    pub output_path: Option<PathBuf>,
}

impl VideoFile {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            analysis: None,
            resolution: None,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            selected_audio: Vec::new(),
            selected_subtitles: Vec::new(),
            status: FileStatus::Pending,
            output_path: None,
        }
    }

    pub fn filename(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    pub fn generate_output_path(&mut self) {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        let parent = self.path.parent().unwrap_or(std::path::Path::new("."));
        self.output_path = Some(parent.join(format!("{}_av1.mkv", stem)));
    }

    pub fn select_all_tracks(&mut self) {
        self.selected_audio = self.audio_tracks.iter().map(|t| t.index).collect();
        self.selected_subtitles = self.subtitle_tracks.iter().map(|t| t.index).collect();
    }

    pub fn toggle_audio(&mut self, index: usize) {
        if self.selected_audio.contains(&index) {
            self.selected_audio.retain(|&i| i != index);
        } else {
            self.selected_audio.push(index);
            self.selected_audio.sort();
        }
    }

    pub fn toggle_subtitle(&mut self, index: usize) {
        if self.selected_subtitles.contains(&index) {
            self.selected_subtitles.retain(|&i| i != index);
        } else {
            self.selected_subtitles.push(index);
            self.selected_subtitles.sort();
        }
    }

    pub fn is_dolby_vision(&self) -> bool {
        matches!(
            self.resolution,
            Some(Resolution::HD1080pDV) | Some(Resolution::UHD2160pDV)
        )
    }

    pub fn resolution_string(&self) -> String {
        match &self.analysis {
            Some(a) => format!("{}x{}", a.width, a.height),
            None => "Unknown".to_string(),
        }
    }

    pub fn hdr_string(&self) -> &'static str {
        match &self.resolution {
            Some(Resolution::HD1080pHDR) | Some(Resolution::UHD2160pHDR) => "HDR",
            Some(Resolution::HD1080pDV) | Some(Resolution::UHD2160pDV) => "Dolby Vision",
            _ => "SDR",
        }
    }
}

pub fn is_video_file(path: &std::path::Path) -> bool {
    let extensions = [
        "mp4", "mkv", "avi", "mov", "webm", "m4v", "ts", "wmv", "flv",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| extensions.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
