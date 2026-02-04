use crate::analyzer::metadata::{HdrType, VideoMetadata};
use crate::error::AppError;
use crate::tracks::{AudioTrack, SubtitleTrack};
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

/// Full analysis result with all tracks
#[derive(Debug)]
pub struct AnalysisResult {
    pub metadata: VideoMetadata,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
}

/// Analyze a video file using ffprobe
pub fn analyze(input_path: &str) -> Result<AnalysisResult, AppError> {
    let metadata = analyze_video_stream(input_path)?;
    let (audio_tracks, subtitle_tracks) = analyze_tracks(input_path)?;

    Ok(AnalysisResult {
        metadata,
        audio_tracks,
        subtitle_tracks,
    })
}

/// Analyze the primary video stream
fn analyze_video_stream(input_path: &str) -> Result<VideoMetadata, AppError> {
    let args = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,pix_fmt,color_primaries,color_transfer,color_space,codec_name,r_frame_rate,avg_frame_rate,bit_rate,side_data_list",
        "-show_entries",
        "format=duration,bit_rate",
        "-of",
        "json",
        input_path,
    ];

    let output = run_ffprobe(&args)?;
    let data: FfprobeOutput = serde_json::from_str(&output)
        .map_err(|e| AppError::Analysis(format!("Failed to parse ffprobe output: {}", e)))?;

    let stream = data
        .streams
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Analysis("No video stream found".to_string()))?;

    // Check for Dolby Vision
    let is_dolby_vision = stream
        .side_data_list
        .as_ref()
        .map(|list| list.iter().any(|v| v.to_string().contains("Dolby Vision")))
        .unwrap_or(false);

    // Determine HDR type
    let hdr_type = if is_dolby_vision {
        HdrType::DolbyVision
    } else {
        match stream.color_transfer.as_deref() {
            Some("smpte2084") => HdrType::Pq,
            Some("arib-std-b67") => HdrType::Hlg,
            _ => HdrType::Sdr,
        }
    };

    // Parse frame rate
    let (frame_rate_num, frame_rate_den) = parse_frame_rate(
        stream
            .r_frame_rate
            .as_deref()
            .or(stream.avg_frame_rate.as_deref()),
    );

    // Parse duration
    let duration_secs = data
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);

    // Parse bitrate from format
    let bitrate = data
        .format
        .as_ref()
        .and_then(|f| f.bit_rate.as_deref())
        .and_then(|b| b.parse::<u64>().ok())
        .or_else(|| {
            stream
                .bit_rate
                .as_deref()
                .and_then(|b| b.parse::<u64>().ok())
        });

    Ok(VideoMetadata {
        width: stream.width,
        height: stream.height,
        hdr_type,
        codec_name: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
        pixel_format: stream.pix_fmt,
        frame_rate_num,
        frame_rate_den,
        duration_secs,
        bitrate,
    })
}

/// Parse frame rate from ffprobe format
fn parse_frame_rate(rate_str: Option<&str>) -> (u32, u32) {
    rate_str
        .and_then(|s| {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 2 {
                let num = parts[0].parse::<u32>().ok()?;
                let den = parts[1].parse::<u32>().ok()?;
                if den > 0 {
                    return Some((num, den));
                }
            }
            None
        })
        .unwrap_or((0, 1))
}

/// Analyze audio and subtitle tracks
fn analyze_tracks(input_path: &str) -> Result<(Vec<AudioTrack>, Vec<SubtitleTrack>), AppError> {
    let args = [
        "-v",
        "error",
        "-show_entries",
        "stream=index,codec_type,codec_name,channels,bit_rate,sample_rate:stream_tags=language,title",
        "-select_streams",
        "a",
        "-of",
        "json",
        input_path,
    ];

    let output = run_ffprobe(&args)?;
    let audio_data: AllStreamsOutput = serde_json::from_str(&output)
        .map_err(|e| AppError::Analysis(format!("Failed to parse ffprobe audio output: {}", e)))?;

    let args_sub = [
        "-v",
        "error",
        "-show_entries",
        "stream=index,codec_type,codec_name:stream_tags=language,title",
        "-select_streams",
        "s",
        "-of",
        "json",
        input_path,
    ];

    let output_sub = run_ffprobe(&args_sub)?;
    let sub_data: AllStreamsOutput = serde_json::from_str(&output_sub).map_err(|e| {
        AppError::Analysis(format!("Failed to parse ffprobe subtitle output: {}", e))
    })?;

    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();

    for (audio_index, stream) in audio_data.streams.into_iter().enumerate() {
        audio_tracks.push(AudioTrack {
            index: audio_index,
            language: stream.tags.as_ref().and_then(|t| t.language.clone()),
            codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
            channels: stream.channels.unwrap_or(2),
            title: stream.tags.as_ref().and_then(|t| t.title.clone()),
            bitrate: stream.bit_rate.and_then(|b| b.parse::<u64>().ok()),
            sample_rate: stream.sample_rate.and_then(|s| s.parse::<u32>().ok()),
        });
    }

    for (subtitle_index, stream) in sub_data.streams.into_iter().enumerate() {
        subtitle_tracks.push(SubtitleTrack {
            index: subtitle_index,
            language: stream.tags.as_ref().and_then(|t| t.language.clone()),
            codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
            title: stream.tags.as_ref().and_then(|t| t.title.clone()),
            forced: false,
        });
    }

    Ok((audio_tracks, subtitle_tracks))
}

/// Run ffprobe with arguments
fn run_ffprobe(args: &[&str]) -> Result<String, AppError> {
    let output = Command::new("ffprobe")
        .args(args)
        .output()
        .map_err(|e| AppError::Analysis(format!("Failed to execute ffprobe: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Analysis(format!("ffprobe failed: {}", stderr)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// JSON deserialization structures

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<VideoStream>,
    format: Option<FormatInfo>,
}

#[derive(Debug, Deserialize)]
struct FormatInfo {
    duration: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
struct VideoStream {
    width: u32,
    height: u32,
    codec_name: Option<String>,
    pix_fmt: Option<String>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_space: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    bit_rate: Option<String>,
    side_data_list: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct AllStreamsOutput {
    streams: Vec<RawStream>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
struct RawStream {
    index: Option<usize>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    channels: Option<u16>,
    bit_rate: Option<String>,
    sample_rate: Option<String>,
    tags: Option<StreamTags>,
}

#[derive(Debug, Deserialize)]
struct StreamTags {
    language: Option<String>,
    title: Option<String>,
}
