//! Analysis Module
//!
//! Video file analysis using ffprobe.

use crate::data::{AudioTrack, SubtitleTrack, VideoAnalysis};
use crate::error::AppError;
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

/// Full analysis result with all tracks
#[derive(Debug)]
pub struct AnalysisResult {
    pub video: VideoAnalysis,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
}

/// Analyze a video file
pub fn analyze(input_path: &str) -> Result<AnalysisResult, AppError> {
    // Get video stream info
    let video = analyze_video_stream(input_path)?;

    // Get audio and subtitle tracks
    let (audio_tracks, subtitle_tracks) = analyze_tracks(input_path)?;

    Ok(AnalysisResult {
        video,
        audio_tracks,
        subtitle_tracks,
    })
}

/// Analyze the primary video stream
fn analyze_video_stream(input_path: &str) -> Result<VideoAnalysis, AppError> {
    let args = [
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

    let output = run_ffprobe(&args)?;
    let data: VideoStreamOutput =
        serde_json::from_str(&output).map_err(|e| AppError::CommandExecution {
            message: format!("Failed to parse ffprobe output: {}", e),
        })?;

    let stream = data
        .streams
        .into_iter()
        .next()
        .ok_or_else(|| AppError::CommandExecution {
            message: "No video stream found".to_string(),
        })?;

    let is_hdr = matches!(
        stream.color_transfer.as_deref(),
        Some("smpte2084") | Some("arib-std-b67")
    );

    let is_dolby_vision = stream
        .side_data_list
        .as_ref()
        .map(|list| list.iter().any(|v| v.to_string().contains("Dolby Vision")))
        .unwrap_or(false);

    Ok(VideoAnalysis {
        width: stream.width,
        height: stream.height,
        is_hdr,
        is_dolby_vision,
        color_transfer: stream.color_transfer,
    })
}

/// Analyze audio and subtitle tracks
fn analyze_tracks(input_path: &str) -> Result<(Vec<AudioTrack>, Vec<SubtitleTrack>), AppError> {
    let args = [
        "-v",
        "error",
        "-show_entries",
        "stream=index,codec_type,codec_name,channels:stream_tags=language,title",
        "-of",
        "json",
        input_path,
    ];

    let output = run_ffprobe(&args)?;
    let data: AllStreamsOutput =
        serde_json::from_str(&output).map_err(|e| AppError::CommandExecution {
            message: format!("Failed to parse ffprobe output: {}", e),
        })?;

    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();
    let mut audio_index = 0;
    let mut subtitle_index = 0;

    for stream in data.streams {
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

    Ok((audio_tracks, subtitle_tracks))
}

/// Run ffprobe with arguments
fn run_ffprobe(args: &[&str]) -> Result<String, AppError> {
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

// JSON deserialization structures

#[derive(Debug, Deserialize)]
struct VideoStreamOutput {
    streams: Vec<VideoStream>,
}

#[derive(Debug, Deserialize)]
struct VideoStream {
    width: u32,
    height: u32,
    #[allow(dead_code)]
    pix_fmt: Option<String>,
    #[allow(dead_code)]
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    #[allow(dead_code)]
    color_space: Option<String>,
    side_data_list: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct AllStreamsOutput {
    streams: Vec<RawStream>,
}

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
