use crate::data::{AudioTrack, SubtitleTrack};
use crate::error::AppError;
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

/// Video resolutions enum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolution {
    HD1080p,
    HD1080pHDR,
    HD1080pDV,
    UHD2160p,
    UHD2160pHDR,
    UHD2160pDV,
}

#[allow(unused)]
#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    pub width: u32,
    pub height: u32,
    pix_fmt: String,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_space: Option<String>,
    side_data_list: Option<Vec<Value>>,
}

impl AnalysisResult {
    pub fn is_hdr(&self) -> bool {
        matches!(
            self.color_transfer.as_deref(),
            Some("smpte2084") | Some("arib-std-b67")
        )
    }

    pub fn is_dolby_vision(&self) -> bool {
        self.side_data_list
            .as_ref()
            .map(|list| list.iter().any(|v| v.to_string().contains("Dolby Vision")))
            .unwrap_or(false)
    }

    /// Get the color transfer characteristic
    pub fn color_transfer(&self) -> Option<&str> {
        self.color_transfer.as_deref()
    }

    pub fn classify_video(&self) -> Result<Resolution, AppError> {
        let is_4k = self.width >= 3000 || self.height >= 1800;
        let hdr = self.is_hdr();
        let dv = self.is_dolby_vision();

        Ok(match (is_4k, hdr, dv) {
            (false, false, false) => Resolution::HD1080p,
            (false, true, false) => Resolution::HD1080pHDR,
            (false, _, true) => Resolution::HD1080pDV,
            (true, false, false) => Resolution::UHD2160p,
            (true, true, false) => Resolution::UHD2160pHDR,
            (true, _, true) => Resolution::UHD2160pDV,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct AnalysisOutput {
    pub streams: Vec<AnalysisResult>,
}

/// Raw stream data from ffprobe for all stream types
#[derive(Debug, Deserialize)]
struct RawStream {
    #[allow(dead_code)]
    index: usize,
    codec_type: String,
    codec_name: Option<String>,
    channels: Option<u16>,
    tags: Option<StreamTags>,
}

#[derive(Debug, Deserialize)]
struct StreamTags {
    language: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FullProbeOutput {
    streams: Vec<RawStream>,
}

/// Full analysis result including all tracks
#[derive(Debug)]
pub struct FullAnalysis {
    pub video: AnalysisResult,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
}

/// Analyze a video file and extract all track information
pub fn analyze(input_path: &str) -> Result<FullAnalysis, AppError> {
    let video_args = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,pix_fmt,color_primaries,color_transfer,color_space,side_data_list",
        "-of",
        "json",
        input_path,
    ];

    let video_output = execute_ffprobe(&video_args)?;
    let video_data: AnalysisOutput =
        serde_json::from_str(&video_output).map_err(|e| AppError::CommandExecution {
            message: format!("Failed to parse video ffprobe output: {}", e),
        })?;

    let video =
        video_data
            .streams
            .into_iter()
            .next()
            .ok_or_else(|| AppError::CommandExecution {
                message: "No video stream found".to_string(),
            })?;

    // Get all streams for audio and subtitle info
    let all_args = [
        "-v",
        "error",
        "-show_entries",
        "stream=index,codec_type,codec_name,channels:stream_tags=language,title",
        "-of",
        "json",
        input_path,
    ];

    let all_output = execute_ffprobe(&all_args)?;
    let all_data: FullProbeOutput =
        serde_json::from_str(&all_output).map_err(|e| AppError::CommandExecution {
            message: format!("Failed to parse streams ffprobe output: {}", e),
        })?;

    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();
    let mut audio_index = 0;
    let mut subtitle_index = 0;

    for stream in all_data.streams {
        match stream.codec_type.as_str() {
            "audio" => {
                audio_tracks.push(AudioTrack {
                    index: audio_index,
                    language: stream.tags.as_ref().and_then(|t| t.language.clone()),
                    codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
                    channels: stream.channels.unwrap_or(2),
                    title: stream.tags.as_ref().and_then(|t| t.title.clone()),
                });
                audio_index += 1;
            }
            "subtitle" => {
                subtitle_tracks.push(SubtitleTrack {
                    index: subtitle_index,
                    language: stream.tags.as_ref().and_then(|t| t.language.clone()),
                    codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
                    title: stream.tags.as_ref().and_then(|t| t.title.clone()),
                });
                subtitle_index += 1;
            }
            _ => {}
        }
    }

    Ok(FullAnalysis {
        video,
        audio_tracks,
        subtitle_tracks,
    })
}

fn execute_ffprobe(args: &[&str]) -> Result<String, AppError> {
    let output =
        Command::new("ffprobe")
            .args(args)
            .output()
            .map_err(|e| AppError::CommandExecution {
                message: format!("Failed to execute ffprobe: {}", e),
            })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::CommandExecution {
            message: format!("ffprobe failed: {}", stderr),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
