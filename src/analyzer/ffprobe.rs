use crate::analyzer::metadata::{Hdr10StaticMetadata, HdrType, VideoMetadata};
use crate::error::AppError;
use crate::tracks::{AudioTrack, SubtitleTrack};
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const FFPROBE_TIMEOUT: Duration = Duration::from_mins(2);
const FFPROBE_STDOUT_LIMIT: u64 = 16 * 1024 * 1024;
const FFPROBE_STDERR_LIMIT: u64 = 4 * 1024 * 1024;

/// Full analysis result with all tracks
#[derive(Debug)]
pub struct AnalysisResult {
    pub metadata: VideoMetadata,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub source_identity: crate::queue::SourceIdentity,
}

/// Analyze a video file using ffprobe
pub fn analyze(input_path: &str, cancel: &AtomicBool) -> Result<AnalysisResult, AppError> {
    let source_identity = crate::queue::SourceIdentity::from_path(input_path)
        .map_err(|e| AppError::Analysis(format!("Could not inspect source: {e}")))?;
    let metadata = analyze_video_stream(input_path, cancel)?;
    let (audio_tracks, subtitle_tracks) = analyze_tracks(input_path, cancel)?;
    if !source_identity.matches_path(input_path) {
        return Err(AppError::Analysis(
            "Source changed while it was being analyzed; add it again".to_string(),
        ));
    }

    Ok(AnalysisResult {
        metadata,
        audio_tracks,
        subtitle_tracks,
        source_identity,
    })
}

/// Analyze the primary video stream
fn analyze_video_stream(input_path: &str, cancel: &AtomicBool) -> Result<VideoMetadata, AppError> {
    let args = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        // `stream_side_data_list` must be selected as its own section:
        // `stream=side_data_list` yields entries with no fields on FFmpeg 7+
        "-show_entries",
        "stream=width,height,pix_fmt,color_primaries,color_transfer,color_space,codec_name,r_frame_rate,avg_frame_rate,bit_rate,duration:stream_side_data_list:format=duration,bit_rate",
        "-of",
        "json",
        input_path,
    ];

    let output = run_ffprobe(&args, cancel)?;
    let data: FfprobeOutput = serde_json::from_str(&output)
        .map_err(|e| AppError::Analysis(format!("Failed to parse ffprobe output: {e}")))?;

    let stream = data
        .streams
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Analysis("No video stream found".to_string()))?;

    // Check for Dolby Vision by inspecting the side_data_type field
    let dovi_entry = stream.side_data_list.as_ref().and_then(|list| {
        list.iter().find(|v| {
            v.get("side_data_type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| {
                    t.eq_ignore_ascii_case("DOVI configuration record")
                        || t.contains("Dolby Vision")
                })
        })
    });
    let dv_profile = dovi_entry.and_then(|v| {
        v.get("dv_profile")
            .and_then(serde_json::Value::as_u64)
            .and_then(|p| u8::try_from(p).ok())
    });

    // Determine HDR type
    let hdr_type = if dovi_entry.is_some() {
        HdrType::DolbyVision
    } else {
        match stream.color_transfer.as_deref() {
            Some("smpte2084") => HdrType::Pq,
            Some("arib-std-b67") => HdrType::Hlg,
            _ => HdrType::Sdr,
        }
    };

    // HDR10 static metadata: prefer container/stream-level side data; for
    // PQ/DV sources without it, probe the first frame (SEI-carried metadata)
    let mut hdr10_static = stream
        .side_data_list
        .as_deref()
        .and_then(parse_hdr10_static);
    if hdr10_static.is_none() && matches!(hdr_type, HdrType::Pq | HdrType::DolbyVision) {
        hdr10_static = probe_frame_hdr10_static(input_path, cancel);
    }

    // Parse frame rate
    let (frame_rate_num, frame_rate_den) = parse_frame_rate(
        stream
            .r_frame_rate
            .as_deref()
            .or(stream.avg_frame_rate.as_deref()),
    );

    // Parse duration — prefer format-level, fall back to stream-level
    let stream_duration = stream
        .duration
        .as_deref()
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|&d| d > 0.0);
    let duration_secs = data
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|&d| d > 0.0)
        .or(stream_duration)
        .unwrap_or(0.0);

    Ok(VideoMetadata {
        width: stream.width,
        height: stream.height,
        hdr_type,
        dv_profile,
        hdr10_static,
        codec_name: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
        frame_rate_num,
        frame_rate_den,
        duration_secs,
    })
}

/// Parse an ffprobe rational like `"35400/50000"` (or a plain number) to f64.
fn parse_rational(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    let s = v.as_str()?;
    if let Some((num, den)) = s.split_once('/') {
        let num = num.trim().parse::<f64>().ok()?;
        let den = den.trim().parse::<f64>().ok()?;
        if den != 0.0 {
            return Some(num / den);
        }
        return None;
    }
    s.trim().parse::<f64>().ok()
}

/// Extract HDR10 static metadata from a ffprobe `side_data_list`, if present.
fn parse_hdr10_static(side_data: &[Value]) -> Option<Hdr10StaticMetadata> {
    let mastering = side_data.iter().find(|v| {
        v.get("side_data_type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.eq_ignore_ascii_case("Mastering display metadata"))
    })?;

    let coord = |key: &str| mastering.get(key).and_then(parse_rational);
    let red = (coord("red_x")?, coord("red_y")?);
    let green = (coord("green_x")?, coord("green_y")?);
    let blue = (coord("blue_x")?, coord("blue_y")?);
    let white_point = (coord("white_point_x")?, coord("white_point_y")?);
    let max_luminance = coord("max_luminance")?;
    let min_luminance = coord("min_luminance")?;

    let cll = side_data.iter().find(|v| {
        v.get("side_data_type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.eq_ignore_ascii_case("Content light level metadata"))
    });
    let cll_val = |key: &str| {
        cll.and_then(|v| v.get(key))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };

    Some(Hdr10StaticMetadata {
        red,
        green,
        blue,
        white_point,
        max_luminance,
        min_luminance,
        max_cll: cll_val("max_content"),
        max_fall: cll_val("max_average"),
    })
}

/// Probe the first video frame for SEI-carried HDR10 static metadata
/// (sources that don't expose it at container level). Best-effort.
fn probe_frame_hdr10_static(input_path: &str, cancel: &AtomicBool) -> Option<Hdr10StaticMetadata> {
    let args = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-read_intervals",
        "%+#1",
        "-show_entries",
        "frame_side_data_list",
        "-of",
        "json",
        input_path,
    ];

    let output = run_ffprobe(&args, cancel).ok()?;
    let data: FramesOutput = serde_json::from_str(&output).ok()?;
    data.frames
        .iter()
        .filter_map(|f| f.side_data_list.as_deref())
        .find_map(parse_hdr10_static)
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
fn analyze_tracks(
    input_path: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<AudioTrack>, Vec<SubtitleTrack>), AppError> {
    let args = [
        "-v",
        "error",
        "-show_entries",
        "stream=index,codec_type,codec_name,channels,channel_layout,bit_rate,sample_rate:stream_tags=language,title",
        "-select_streams",
        "a",
        "-of",
        "json",
        input_path,
    ];

    let output = run_ffprobe(&args, cancel)?;
    let audio_data: AllStreamsOutput = serde_json::from_str(&output)
        .map_err(|e| AppError::Analysis(format!("Failed to parse ffprobe audio output: {e}")))?;

    let args_sub = [
        "-v",
        "error",
        "-show_entries",
        "stream=index,codec_type,codec_name:stream_tags=language,title:stream_disposition=forced",
        "-select_streams",
        "s",
        "-of",
        "json",
        input_path,
    ];

    let output_sub = run_ffprobe(&args_sub, cancel)?;
    let sub_data: AllStreamsOutput = serde_json::from_str(&output_sub)
        .map_err(|e| AppError::Analysis(format!("Failed to parse ffprobe subtitle output: {e}")))?;

    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();

    for (audio_index, stream) in audio_data.streams.into_iter().enumerate() {
        audio_tracks.push(AudioTrack {
            index: audio_index,
            language: stream.tags.as_ref().and_then(|t| t.language.clone()),
            codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
            channels: stream.channels,
            channel_layout: stream.channel_layout,
            title: stream.tags.as_ref().and_then(|t| t.title.clone()),
            bitrate: stream.bit_rate.and_then(|b| b.parse::<u64>().ok()),
            sample_rate: stream.sample_rate.and_then(|s| s.parse::<u32>().ok()),
        });
    }

    for (subtitle_index, stream) in sub_data.streams.into_iter().enumerate() {
        let forced = stream
            .disposition
            .as_ref()
            .and_then(|d| d.forced)
            .unwrap_or(0)
            != 0;

        subtitle_tracks.push(SubtitleTrack {
            index: subtitle_index,
            language: stream.tags.as_ref().and_then(|t| t.language.clone()),
            codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
            title: stream.tags.as_ref().and_then(|t| t.title.clone()),
            forced,
        });
    }

    Ok((audio_tracks, subtitle_tracks))
}

/// Run ffprobe with arguments
fn run_ffprobe(args: &[&str], cancel: &AtomicBool) -> Result<String, AppError> {
    let mut command = Command::new("ffprobe");
    command.args(args);
    let output = run_command(&mut command, cancel, FFPROBE_TIMEOUT)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Analysis(format!("ffprobe failed: {stderr}")));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a probe without allowing a broken file or network mount to hang the UI.
fn run_command(
    command: &mut Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<std::process::Output, AppError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Analysis("Cancelled".to_string()));
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Analysis(format!("Failed to execute ffprobe: {e}")))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = std::thread::spawn(move || read_capped(&mut stdout, FFPROBE_STDOUT_LIMIT));
    let stderr_reader = std::thread::spawn(move || read_capped(&mut stderr, FFPROBE_STDERR_LIMIT));
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || started.elapsed() >= timeout {
            let cancelled = cancel.load(Ordering::Relaxed);
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(AppError::Analysis(if cancelled {
                "Cancelled".to_string()
            } else {
                "ffprobe timed out".to_string()
            }));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(AppError::Analysis(format!(
                    "Failed to wait for ffprobe: {e}"
                )));
            }
        }
    };
    let (stdout, stdout_limited) = stdout_reader
        .join()
        .map_err(|_| AppError::Analysis("ffprobe stdout reader panicked".to_string()))?
        .map_err(|e| AppError::Analysis(format!("Failed to read ffprobe stdout: {e}")))?;
    let (stderr, stderr_limited) = stderr_reader
        .join()
        .map_err(|_| AppError::Analysis("ffprobe stderr reader panicked".to_string()))?
        .map_err(|e| AppError::Analysis(format!("Failed to read ffprobe stderr: {e}")))?;
    if stdout_limited || stderr_limited {
        return Err(AppError::Analysis(
            "ffprobe produced too much output; refusing to exhaust memory".to_string(),
        ));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn read_capped(reader: &mut impl Read, limit: u64) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    let exceeded = bytes.len() as u64 > limit;
    if exceeded {
        bytes.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    Ok((bytes, exceeded))
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
}

#[derive(Debug, Deserialize)]
struct VideoStream {
    width: u32,
    height: u32,
    codec_name: Option<String>,
    color_transfer: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    side_data_list: Option<Vec<Value>>,
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FramesOutput {
    #[serde(default)]
    frames: Vec<FrameInfo>,
}

#[derive(Debug, Deserialize)]
struct FrameInfo {
    side_data_list: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct AllStreamsOutput {
    streams: Vec<RawStream>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    codec_name: Option<String>,
    channels: Option<u16>,
    channel_layout: Option<String>,
    bit_rate: Option<String>,
    sample_rate: Option<String>,
    tags: Option<StreamTags>,
    disposition: Option<StreamDisposition>,
}

#[derive(Debug, Deserialize)]
struct StreamTags {
    language: Option<String>,
    title: Option<String>,
}

/// Stream disposition flags (only the forced flag is currently used)
#[derive(Debug, Deserialize)]
struct StreamDisposition {
    forced: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hdr10_static_from_real_ffprobe_json() {
        // Captured from `ffprobe -show_entries stream_side_data_list` (FFmpeg 8)
        let side_data: Vec<Value> = serde_json::from_str(
            r#"[
                {"side_data_type": "Content light level metadata",
                 "max_content": 1000, "max_average": 400},
                {"side_data_type": "Mastering display metadata",
                 "red_x": "35400/50000", "red_y": "14600/50000",
                 "green_x": "8500/50000", "green_y": "39850/50000",
                 "blue_x": "6550/50000", "blue_y": "2300/50000",
                 "white_point_x": "15635/50000", "white_point_y": "16450/50000",
                 "min_luminance": "50/10000", "max_luminance": "10000000/10000"}
            ]"#,
        )
        .unwrap();

        let hdr10 = parse_hdr10_static(&side_data).unwrap();
        assert!((hdr10.red.0 - 0.708).abs() < 1e-9);
        assert!((hdr10.green.1 - 0.797).abs() < 1e-9);
        assert!((hdr10.max_luminance - 1000.0).abs() < 1e-9);
        assert!((hdr10.min_luminance - 0.005).abs() < 1e-9);
        assert_eq!(hdr10.max_cll, 1000);
        assert_eq!(hdr10.max_fall, 400);
        assert_eq!(hdr10.svt_content_light().as_deref(), Some("1000,400"));
        assert_eq!(
            hdr10.svt_mastering_display(),
            "G(0.17000,0.79700)B(0.13100,0.04600)R(0.70800,0.29200)\
             WP(0.31270,0.32900)L(1000.0000,0.0050)"
        );
    }

    #[test]
    fn missing_mastering_display_yields_none() {
        let side_data: Vec<Value> = serde_json::from_str(
            r#"[{"side_data_type": "DOVI configuration record", "dv_profile": 8}]"#,
        )
        .unwrap();
        assert!(parse_hdr10_static(&side_data).is_none());
    }

    #[test]
    fn probe_output_reader_stops_at_its_memory_limit() {
        let mut input = std::io::Cursor::new(b"12345");
        let (bytes, exceeded) = read_capped(&mut input, 4).unwrap();
        assert_eq!(bytes, b"1234");
        assert!(exceeded);
    }

    #[cfg(unix)]
    #[test]
    fn probe_process_is_killed_at_its_deadline() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 5"]);
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let error = run_command(&mut command, &cancel, Duration::from_millis(10)).unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
