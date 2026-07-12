use crate::analyzer::{DvMode, VideoMetadata};
use crate::config::TrackPresetConfig;
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
    Encoding { progress: f64 },
    /// Running VMAF quality check after encoding
    Verifying,
    /// Successfully encoded (no VMAF run)
    Done,
    /// Encoded with VMAF score meeting threshold
    DoneWithVmaf { score: f64 },
    /// Encoded successfully but VMAF check could not run
    DoneVmafFailed { reason: String },
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
    pub remux_only: bool,
    /// Dolby Vision handling; `None` until the user has chosen (DV sources only)
    pub dv_mode: Option<DvMode>,
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
            remux_only: false,
            dv_mode: None,
        }
    }

    /// Get the filename
    pub fn filename(&self) -> String {
        self.path.file_name().map_or_else(
            || "Unknown".to_string(),
            |n| n.to_string_lossy().to_string(),
        )
    }

    /// Get the resolution string
    pub fn resolution_string(&self) -> String {
        self.metadata
            .as_ref()
            .map_or_else(|| "Unknown".to_string(), VideoMetadata::resolution_string)
    }

    /// Get the HDR string
    pub fn hdr_string(&self) -> &str {
        self.metadata
            .as_ref()
            .map_or("Unknown", VideoMetadata::hdr_string)
    }

    /// Generate the output path based on config
    pub fn generate_output_path(&mut self, output_config: &crate::config::OutputConfig) {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        let default_parent = || self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let parent = if output_config.same_directory {
            default_parent()
        } else if let Some(ref dir) = output_config.output_directory {
            std::path::PathBuf::from(dir)
        } else {
            default_parent()
        };

        let suffix = if self.remux_only {
            "_remux".to_string()
        } else {
            output_config.suffix.clone()
        };

        let container = if self.remux_only {
            self.path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(&output_config.container)
                .to_string()
        } else {
            output_config.container.clone()
        };

        self.output_path = Some(parent.join(format!("{stem}{suffix}.{container}")));
    }

    /// Calculate size reduction if both sizes are known
    pub fn size_reduction(&self) -> Option<(u64, f64)> {
        match (self.source_size, self.output_size) {
            (Some(source), Some(output)) if source > 0 => {
                let saved = source.saturating_sub(output);
                // Use u128 to avoid u64→f64 precision lint: (saved*100)/source is in [0,100]
                let percent_int =
                    u32::try_from(u128::from(saved) * 100 / u128::from(source)).unwrap_or(100);
                let percent = f64::from(percent_int);
                Some((saved, percent))
            }
            _ => None,
        }
    }
}

/// Recursively collect video files under `dir`, skipping symlinks.
pub fn collect_video_files(dir: &Path, paths: &mut Vec<PathBuf>) {
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
pub fn auto_select_tracks(job: &mut EncodingJob, config: &TrackPresetConfig) {
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

/// Check if a path is a video file
pub fn is_video_file(path: &Path) -> bool {
    const VIDEO_EXTENSIONS: [&str; 10] = [
        "mp4", "mkv", "avi", "mov", "webm", "m4v", "ts", "m2ts", "wmv", "flv",
    ];

    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        VIDEO_EXTENSIONS
            .iter()
            .any(|&ext| ext.eq_ignore_ascii_case(e))
    })
}
