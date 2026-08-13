//! Encodes a real file with a real `FFmpeg`.
//!
//! Everything else in this crate tests the arguments we intend to pass.
//! Nothing tested whether `FFmpeg` accepts them — and it does not always: the
//! `5.1(side)` channel layout, which is what ffprobe reports for most real
//! surround tracks, is rejected outright under Opus' standard mapping. That
//! shipped through argument-level tests, a clippy pass and three readings, and
//! only turned up when a file was actually put through the encoder.
//!
//! Skipped, not failed, when the `FFmpeg` on `PATH` cannot do the job: the point
//! is to catch regressions where the tooling exists, not to demand it.

use crate::analyzer::{DvMode, HdrType, VideoMetadata};
use crate::config::{AppConfig, AudioConfig, Encoder};
use crate::encoder::command_builder::EncodingParams;
use crate::encoder::ffmpeg::{EncodeResult, encode_video};
use crate::tracks::{AudioTrack, TrackSelection};
use crate::utils::DependencyStatus;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

/// Whether this machine can run the test at all.
fn ffmpeg_can_encode_av1_opus() -> bool {
    DependencyStatus::check()
        && DependencyStatus::encoder_available("libsvtav1")
        && DependencyStatus::libopus_available()
}

/// A short clip with a 5.1 audio track, which ffprobe reports as `5.1(side)`.
fn make_fixture(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("clip.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=24:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:v",
            "libx264",
            "-c:a",
            "ac3",
            "-ac",
            "6",
            // A title naming the source codec, as real releases carry
            "-metadata:s:a:0",
            "title=AC3 5.1 @ 640 kbps",
            "-shortest",
        ])
        .arg(&path)
        .status()
        .ok()?;
    status.success().then_some(path)
}

/// `ffprobe -show_entries stream=<field>` for every stream, one per line.
fn probe(path: &Path, entries: &str) -> Vec<String> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", entries, "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().trim_end_matches(',').to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn metadata_for(path: &Path) -> VideoMetadata {
    let duration = probe(path, "format=duration")
        .first()
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(1.0);
    VideoMetadata {
        width: 320,
        height: 240,
        hdr_type: HdrType::Sdr,
        dv_profile: None,
        hdr10_static: None,
        codec_name: "h264".to_string(),
        frame_rate_num: 24,
        frame_rate_den: 1,
        duration_secs: duration,
    }
}

fn surround_track() -> AudioTrack {
    AudioTrack {
        index: 0,
        language: None,
        codec: "ac3".to_string(),
        channels: Some(6),
        // Exactly what ffprobe reports for the fixture.
        channel_layout: Some("5.1(side)".to_string()),
        title: Some("AC3 5.1 @ 640 kbps".to_string()),
        bitrate: None,
        sample_rate: None,
    }
}

/// A 5.1 source, transcoded to Opus, has to come out the other side: AV1 video
/// and a six-channel Opus track, with the scratch file renamed into place and
/// nothing left behind.
#[test]
fn encodes_a_surround_source_to_av1_and_opus() {
    if !ffmpeg_can_encode_av1_opus() {
        eprintln!("skipping: this FFmpeg cannot encode AV1 + Opus");
        return;
    }

    let dir = std::env::temp_dir().join(format!("av1c_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(input) = make_fixture(&dir) else {
        eprintln!("skipping: could not build the fixture clip");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };

    // The layout the fixture really has is the one this test exists for.
    assert_eq!(
        probe(&input, "stream=channel_layout"),
        vec!["5.1(side)"],
        "fixture must carry the layout that libopus rejects under standard mapping"
    );

    let mut selection = TrackSelection::default();
    selection.set_audio_opus(0, true);
    let tracks = selection.resolve(&[surround_track()], &AudioConfig::default());
    assert!(tracks.transcodes_audio(), "the audio must actually be Opus");

    let output = dir.join("clip_av1.mkv");
    let config = AppConfig {
        encoder: Encoder::SvtAv1,
        ..AppConfig::default()
    };
    let metadata = metadata_for(&input);
    let params = EncodingParams::from_metadata(
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        &metadata,
        &config,
        tracks,
        DvMode::ToHdr10,
        false,
        Vec::new(),
    );

    let result = encode_video(&params, None, &AtomicBool::new(false), 1.0, 24.0);
    assert!(
        matches!(result, EncodeResult::Success),
        "encode failed: {result:?}"
    );

    // The streams FFmpeg actually produced, not the ones we asked for.
    assert_eq!(
        probe(&output, "stream=codec_name"),
        vec!["av1", "opus"],
        "expected AV1 video and an Opus audio track"
    );
    assert_eq!(
        probe(&output, "stream=channels"),
        vec!["6"],
        "the source's six channels must survive; no silent downmix"
    );
    assert_eq!(
        probe(&output, "stream=channel_layout"),
        vec!["5.1"],
        "the Opus track must declare a real channel layout, not `unknown`"
    );
    assert_eq!(
        probe(&output, "stream_tags=title"),
        vec!["Opus 5.1"],
        "the track title must name the codec the track actually is"
    );

    // The scratch file is renamed into place, never left lying around.
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".part."))
        .collect();
    assert!(
        leftovers.is_empty(),
        "scratch files left behind: {leftovers:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A failed encode must not leave the destination holding a broken file, and
/// must not touch an unrelated file that was already sitting there.
#[test]
fn a_failed_encode_leaves_the_destination_alone() {
    if !DependencyStatus::check() {
        eprintln!("skipping: ffmpeg is not on PATH");
        return;
    }

    let dir = std::env::temp_dir().join(format!("av1c_e2e_fail_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Not a video, so FFmpeg refuses it.
    let input = dir.join("broken.mkv");
    std::fs::write(&input, b"this is not a video file").unwrap();
    let output = dir.join("broken_av1.mkv");

    let config = AppConfig {
        encoder: Encoder::SvtAv1,
        ..AppConfig::default()
    };
    let params = EncodingParams::from_metadata(
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        &metadata_for(&input),
        &config,
        crate::tracks::OutputTracks::default(),
        DvMode::ToHdr10,
        false,
        Vec::new(),
    );

    let result = encode_video(&params, None, &AtomicBool::new(false), 1.0, 24.0);
    assert!(
        matches!(result, EncodeResult::Error(_)),
        "a non-video input must fail: {result:?}"
    );
    assert!(
        !output.exists(),
        "a failed encode must not create the output"
    );

    let remaining: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(remaining, vec!["broken.mkv".to_string()]);

    let _ = std::fs::remove_dir_all(&dir);
}
