//! Data Module
//!
//! Core data structures for video files and tracks.

use crate::config::Profile;
use std::path::{Path, PathBuf};

/// HDR type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HdrType {
    /// Standard Dynamic Range
    #[default]
    Sdr,
    /// PQ (Perceptual Quantizer) - HDR10/HDR10+
    Pq,
    /// HLG (Hybrid Log-Gamma)
    Hlg,
    /// Dolby Vision
    DolbyVision,
}

impl HdrType {
    /// Check if this is any HDR format
    pub fn is_hdr(&self) -> bool {
        !matches!(self, HdrType::Sdr)
    }

    /// Get display string for this HDR type
    pub fn display_string(&self) -> &'static str {
        match self {
            HdrType::Sdr => "SDR",
            HdrType::Pq => "HDR10",
            HdrType::Hlg => "HLG",
            HdrType::DolbyVision => "Dolby Vision",
        }
    }
}

/// Audio track information
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

/// Subtitle track information
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

/// Video file status during processing
#[derive(Debug, Clone)]
pub enum FileStatus {
    /// Waiting to be processed
    Pending,
    /// Being analyzed
    Analyzing,
    /// Waiting for track configuration
    AwaitingConfig,
    /// Ready to encode
    Ready,
    /// Currently encoding
    Encoding { progress: f32 },
    /// Successfully encoded
    Done,
    /// Encoded with VMAF score
    DoneWithVmaf { score: f64 },
    /// Skipped (e.g., cancelled)
    Skipped { reason: String },
    /// Error occurred
    Error { message: String },
    /// Encoded but quality below threshold
    QualityWarning { vmaf: f64, threshold: f64 },
}

/// Video analysis result
#[derive(Debug, Clone)]
pub struct VideoAnalysis {
    pub width: u32,
    pub height: u32,
    pub hdr_type: HdrType,
}

impl VideoAnalysis {
    /// Determine the encoding profile based on analysis
    pub fn profile(&self) -> Profile {
        let is_4k = self.width >= 3000 || self.height >= 1800;
        let is_hdr = self.hdr_type.is_hdr();

        match (is_4k, is_hdr) {
            (false, false) => Profile::HD1080p,
            (false, true) => Profile::HD1080pHDR,
            (true, false) => Profile::UHD2160p,
            (true, true) => Profile::UHD2160pHDR,
        }
    }

    /// Get resolution string for display
    pub fn resolution_string(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    /// Get HDR status string for display
    pub fn hdr_string(&self) -> &'static str {
        self.hdr_type.display_string()
    }
}

/// Video file with all metadata
#[derive(Debug, Clone)]
pub struct VideoFile {
    pub path: PathBuf,
    pub analysis: Option<VideoAnalysis>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub selected_audio: Vec<usize>,
    pub selected_subtitles: Vec<usize>,
    pub status: FileStatus,
    pub output_path: Option<PathBuf>,
}

impl VideoFile {
    /// Create a new video file entry
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            analysis: None,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            selected_audio: Vec::new(),
            selected_subtitles: Vec::new(),
            status: FileStatus::Pending,
            output_path: None,
        }
    }

    /// Get the filename
    pub fn filename(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Get the encoding profile based on analysis
    pub fn profile(&self) -> Profile {
        self.analysis
            .as_ref()
            .map(|a| a.profile())
            .unwrap_or_default()
    }

    /// Get the HDR type for this video
    pub fn hdr_type(&self) -> HdrType {
        self.analysis
            .as_ref()
            .map(|a| a.hdr_type)
            .unwrap_or(HdrType::Sdr)
    }

    /// Generate the output path
    pub fn generate_output_path(&mut self) {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        let parent = self.path.parent().unwrap_or(Path::new("."));
        self.output_path = Some(parent.join(format!("{}_av1.mkv", stem)));
    }

    /// Select all available tracks
    pub fn select_all_tracks(&mut self) {
        self.selected_audio = self.audio_tracks.iter().map(|t| t.index).collect();
        self.selected_subtitles = self.subtitle_tracks.iter().map(|t| t.index).collect();
    }

    /// Toggle audio track selection
    pub fn toggle_audio(&mut self, index: usize) {
        if self.selected_audio.contains(&index) {
            self.selected_audio.retain(|&i| i != index);
        } else {
            self.selected_audio.push(index);
            self.selected_audio.sort();
        }
    }

    /// Toggle subtitle track selection
    pub fn toggle_subtitle(&mut self, index: usize) {
        if self.selected_subtitles.contains(&index) {
            self.selected_subtitles.retain(|&i| i != index);
        } else {
            self.selected_subtitles.push(index);
            self.selected_subtitles.sort();
        }
    }

    /// Get resolution string for display
    pub fn resolution_string(&self) -> String {
        self.analysis
            .as_ref()
            .map(|a| a.resolution_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Get HDR status string for display
    pub fn hdr_string(&self) -> &'static str {
        self.analysis
            .as_ref()
            .map(|a| a.hdr_string())
            .unwrap_or("Unknown")
    }
}

/// Check if a path is a video file
pub fn is_video_file(path: &Path) -> bool {
    const VIDEO_EXTENSIONS: [&str; 9] = [
        "mp4", "mkv", "avi", "mov", "webm", "m4v", "ts", "wmv", "flv",
    ];

    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
