use std::path::PathBuf;

/// Audio track informations
#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub index: usize,
    pub language: String,
    pub codec: String,
    pub channels: u16,
    pub sample_rate: i32,
}

/// Video file metadata
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub filepath: PathBuf,
    pub container: String,
    pub video_codec: transcoder::VideoCodec,
    pub resolution: transcoder::VideoResolution,
    pub duration: f64,
    pub audio_tracks: Vec<AudioTrack>,
}

/// Quality preset selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
}

impl QualityPreset {
    // pub fn to_string(&self) -> &'static str {
    //     match self {
    //         QualityPreset::Low => "Low",
    //         QualityPreset::Medium => "Medium",
    //         QualityPreset::High => "High",
    //     }
    // }
}

/// Audio handling mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    CopyAll,
    SelectTracks, // To be implemented
}

/// Output naming mode
// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// // TODO: implement rename
// pub enum OutputNaming {
//     SameAsInput,
//     Rename,
// }

/// Configurations for encoding
#[derive(Debug, Clone)]
pub struct EncodeConfig {
    pub quality: QualityPreset,
    pub no_upscaling: bool,
    pub audio_mode: AudioMode,
    //pub selected_audio_tracks: Vec<usize>,
    pub output_path: Option<PathBuf>,
    //pub output_name: Option<String>,
}

impl Default for EncodeConfig {
    fn default() -> Self {
        Self {
            quality: QualityPreset::Medium,
            no_upscaling: true,
            audio_mode: AudioMode::CopyAll,
            //selected_audio_tracks: Vec::new(),
            output_path: None,
            //output_name: None,
        }
    }
}

/// Encoding progress information
#[derive(Debug, Clone)]
pub struct EncodeProgress {
    pub percentage: f64,
    pub elapsed_time: std::time::Duration,
    pub estimated_time_remaining: Option<std::time::Duration>,
    pub frame_count: usize,
}

impl Default for EncodeProgress {
    fn default() -> Self {
        Self {
            percentage: 0.0,
            elapsed_time: std::time::Duration::ZERO,
            estimated_time_remaining: None,
            frame_count: 0,
        }
    }
}
