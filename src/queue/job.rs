use crate::analyzer::VideoMetadata;
use crate::tracks::{AudioTrack, SubtitleTrack, TrackSelection};
use std::path::{Path, PathBuf};

/// Status of a job in the encoding queue
#[derive(Debug, Clone)]
pub enum JobStatus {
    /// Waiting to be processed
    Pending,
    /// Being analyzed via ffprobe
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
    /// Skipped (e.g., already AV1, cancelled)
    Skipped { reason: String },
    /// Error occurred
    Error { message: String },
    /// Encoded but quality below threshold
    QualityWarning { vmaf: f64, threshold: f64 },
}

/// An encoding job in the queue
#[derive(Debug, Clone)]
pub struct EncodingJob {
    pub path: PathBuf,
    pub metadata: Option<VideoMetadata>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub track_selection: TrackSelection,
    pub status: JobStatus,
    pub output_path: Option<PathBuf>,
    pub crf: Option<u8>,
    pub source_size: Option<u64>,
    pub output_size: Option<u64>,
    pub source_deleted: bool,
    pub source_kept_vmaf: Option<f64>,
}

impl EncodingJob {
    /// Create a new encoding job
    pub fn new(path: PathBuf) -> Self {
        let source_size = std::fs::metadata(&path).ok().map(|m| m.len());
        Self {
            path,
            metadata: None,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            track_selection: TrackSelection::default(),
            status: JobStatus::Pending,
            output_path: None,
            crf: None,
            source_size,
            output_size: None,
            source_deleted: false,
            source_kept_vmaf: None,
        }
    }

    /// Get the filename
    pub fn filename(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Get the resolution string
    pub fn resolution_string(&self) -> String {
        self.metadata
            .as_ref()
            .map(|m| m.resolution_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Get the HDR string
    pub fn hdr_string(&self) -> &str {
        self.metadata
            .as_ref()
            .map(|m| m.hdr_string())
            .unwrap_or("Unknown")
    }

    /// Generate the output path based on config
    pub fn generate_output_path(&mut self, suffix: &str, container: &str) {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        let parent = self.path.parent().unwrap_or(Path::new("."));
        self.output_path = Some(parent.join(format!("{}{}.{}", stem, suffix, container)));
    }

    /// Select all available tracks
    pub fn select_all_tracks(&mut self) {
        self.track_selection =
            TrackSelection::select_all(&self.audio_tracks, &self.subtitle_tracks);
    }

    /// Calculate size reduction if both sizes are known
    pub fn size_reduction(&self) -> Option<(u64, f64)> {
        match (self.source_size, self.output_size) {
            (Some(source), Some(output)) if source > 0 => {
                let saved = source.saturating_sub(output);
                let percent = (saved as f64 / source as f64) * 100.0;
                Some((saved, percent))
            }
            _ => None,
        }
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
