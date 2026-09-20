//! Encodes a real file with a real `FFmpeg`. Every other test in this crate
//! checks the arguments we intend to pass; these check that `FFmpeg` accepts
//! them and produces the streams asked for.
//!
//! Skipped, not failed, when the `FFmpeg` on `PATH` cannot do the job.

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
        dv_bl_compat: None,
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

/// A 5.1 source comes out as AV1 video and a six-channel Opus track, with the
/// scratch file renamed into place and nothing left behind.
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

    // The layout the fixture really has.
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

/// A failed encode leaves no broken file at the destination, and does not
/// touch an unrelated file already sitting there.
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
    // Already in the destination, and no business of this encode.
    let bystander = dir.join("notes.txt");
    std::fs::write(&bystander, b"someone else's file").unwrap();

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

    assert_eq!(std::fs::read(&bystander).unwrap(), b"someone else's file");

    let mut remaining: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    remaining.sort();
    assert_eq!(
        remaining,
        vec!["broken.mkv".to_string(), "notes.txt".to_string()]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Move `moov/udta`, which holds the cover of an `+faststart` MP4, ahead of
/// the tracks.
fn move_cover_first(path: &Path) {
    let data = std::fs::read(path).unwrap();
    let size = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let kind = |at: usize| &data[at + 4..at + 8];
    let mut moov = 0;
    while kind(moov) != b"moov" {
        moov += size(moov);
    }
    let (start, end) = (moov + 8, moov + size(moov));
    let mut children = Vec::new();
    let mut at = start;
    while at < end {
        children.push(at..at + size(at));
        at += size(at);
    }
    children.sort_by_key(|child| match kind(child.start) {
        b"mvhd" => 0,
        b"udta" => 1,
        _ => 2,
    });
    let mut out = data[..start].to_vec();
    for child in children {
        out.extend_from_slice(&data[child]);
    }
    out.extend_from_slice(&data[end..]);
    std::fs::write(path, out).unwrap();
}

/// Cover art stored before the film is neither analyzed nor encoded in its
/// place.
#[test]
fn the_movie_is_encoded_not_its_cover_art() {
    if !(DependencyStatus::check() && DependencyStatus::encoder_available("libsvtav1")) {
        eprintln!("skipping: this FFmpeg cannot encode AV1");
        return;
    }

    let dir = std::env::temp_dir().join(format!("av1c_e2e_cover_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("cover.mp4");
    let built = Command::new("ffmpeg")
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
            "color=c=red:s=64x64:d=0.04",
            "-map",
            "0",
            "-map",
            "1",
            "-c:v:0",
            "libx264",
            "-c:v:1",
            "mjpeg",
            "-disposition:v:1",
            "attached_pic",
            "-movflags",
            "+faststart",
        ])
        .arg(&input)
        .status()
        .is_ok_and(|status| status.success());
    if !built {
        eprintln!("skipping: could not build the fixture clip");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    move_cover_first(&input);
    if probe(&input, "stream=codec_name")
        .first()
        .map(String::as_str)
        != Some("mjpeg")
    {
        eprintln!("skipping: this FFmpeg does not list the moved cover first");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let analysis =
        crate::analyzer::ffprobe::analyze(input.to_str().unwrap(), &AtomicBool::new(false))
            .expect("the fixture analyzes");
    assert_eq!(
        analysis.metadata.width, 320,
        "the film, not the 64x64 cover"
    );

    let output = dir.join("out.mkv");
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
        matches!(result, EncodeResult::Success),
        "encode failed: {result:?}"
    );
    assert_eq!(probe(&output, "stream=codec_name,width"), vec!["av1,320"]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Subtitles that are converted or left out, and cover art the output does
/// not carry, keep the source whatever the VMAF score.
#[test]
fn lost_cover_art_or_subtitles_keep_the_source() {
    if !DependencyStatus::check() {
        eprintln!("skipping: ffmpeg is not on PATH");
        return;
    }

    let dir = std::env::temp_dir().join(format!("av1c_e2e_keep_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(clip) = make_fixture(&dir) else {
        eprintln!("skipping: could not build the fixture clip");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    let cover = dir.join("cover.jpg");
    let covered = dir.join("covered.mkv");
    let font = dir.join("face.ttf");
    let fonted = dir.join("fonted.mkv");
    std::fs::write(&font, b"not a real font, but an attachment").unwrap();
    let ffmpeg = |args: &[&str]| {
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    };
    let built = ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=red:s=64x64",
        "-frames:v",
        "1",
        cover.to_str().unwrap(),
    ]) && ffmpeg(&[
        "-i",
        clip.to_str().unwrap(),
        "-map",
        "0",
        "-c",
        "copy",
        "-attach",
        cover.to_str().unwrap(),
        "-metadata:s:t:0",
        "mimetype=image/jpeg",
        "-metadata:s:t:0",
        "filename=cover.jpg",
        covered.to_str().unwrap(),
    ]) && ffmpeg(&[
        "-i",
        clip.to_str().unwrap(),
        "-map",
        "0",
        "-c",
        "copy",
        "-attach",
        font.to_str().unwrap(),
        "-metadata:s:t:0",
        "mimetype=application/x-truetype-font",
        "-metadata:s:t:0",
        "filename=face.ttf",
        fonted.to_str().unwrap(),
    ]);
    if !built {
        eprintln!("skipping: could not build the fixture clips");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let output = dir.join("out.mkv");
    let reason = |input: &Path, subtitles: Vec<Option<&'static str>>| {
        let tracks = crate::tracks::OutputTracks {
            subtitle_indices: (0..subtitles.len()).collect(),
            ..crate::tracks::OutputTracks::default()
        };
        let params = EncodingParams::from_metadata(
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &metadata_for(input),
            &AppConfig::default(),
            tracks,
            DvMode::ToHdr10,
            false,
            subtitles,
        );
        super::keep_source_reason(&params, DvMode::ToHdr10, &AtomicBool::new(false))
    };

    assert_eq!(reason(&clip, vec![Some("copy")]), None);
    assert!(reason(&clip, vec![Some("mov_text")]).is_some());
    assert!(reason(&clip, vec![None]).is_some());
    assert!(reason(&covered, Vec::new()).is_some());
    // An MKV output carries a font attachment through, and it is no keep
    // reason.
    assert_eq!(reason(&fonted, Vec::new()), None);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A 4:4:4 or 12-bit source is kept whatever the VMAF score; its 4:2:0 8-bit
/// counterpart is not.
#[test]
fn a_4_4_4_or_12_bit_source_is_kept() {
    if !DependencyStatus::check() {
        eprintln!("skipping: ffmpeg is not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!("av1c_e2e_pixfmt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let clip = |name: &str, pix_fmt: &str| {
        let path = dir.join(name);
        let built = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x120:rate=24:duration=1",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                pix_fmt,
            ])
            .arg(&path)
            .status()
            .is_ok_and(|status| status.success());
        assert!(built, "ffmpeg builds a {pix_fmt} ffv1 clip");
        path
    };
    let output = dir.join("out.mkv");
    let reason = |input: &Path| {
        let params = EncodingParams::from_metadata(
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &metadata_for(input),
            &AppConfig::default(),
            crate::tracks::OutputTracks::default(),
            DvMode::ToHdr10,
            false,
            Vec::new(),
        );
        super::keep_source_reason(&params, DvMode::ToHdr10, &AtomicBool::new(false))
    };

    let kept = Some(super::KeepReason::ChromaOrBitDepth);
    assert_eq!(reason(&clip("420.mkv", "yuv420p")), None);
    assert_eq!(reason(&clip("444.mkv", "yuv444p")), kept);
    assert_eq!(reason(&clip("420_12.mkv", "yuv420p12le")), kept);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A one-second video-only clip in `dir`.
fn make_video_fixture(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("video.mkv");
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=24:duration=1",
            "-c:v",
            "libx264",
        ])
        .arg(&path)
        .status()
        .is_ok_and(|status| status.success())
        .then_some(path)
}

/// A Matroska file whose video stops at two seconds while its audio runs to
/// ten reports the video's length as the encoded duration.
#[test]
fn encoded_duration_is_the_video_stream_not_a_longer_audio_track() {
    if !DependencyStatus::check() {
        eprintln!("skipping: ffmpeg/ffprobe not available");
        return;
    }
    let dir = std::env::temp_dir().join(format!("av1c_e2e_short_video_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("short_video.mkv");
    let built = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x120:rate=24:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=10",
            "-c:v",
            "ffv1",
            "-c:a",
            "flac",
        ])
        .arg(&path)
        .status()
        .is_ok_and(|status| status.success());
    assert!(
        built,
        "ffmpeg builds the fixture with its native ffv1 and flac encoders"
    );

    let cancel = AtomicBool::new(false);
    let secs = crate::analyzer::ffprobe::probe_duration_secs(&path.to_string_lossy(), &cancel)
        .expect("ffprobe reports a duration");
    assert!((secs - 2.0).abs() < 0.1, "video duration {secs}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Runs the full pipeline with source deletion on, against a fresh
/// video-only clip in its own directory. `None` when this machine cannot
/// encode AV1 and score VMAF.
fn run_deleting_pipeline(
    name: &str,
    threshold: f64,
    cancel: &std::sync::Arc<AtomicBool>,
    on_before_vmaf: impl FnOnce(&Path, &Path) + Send + 'static,
) -> Option<(super::FullEncodeResult, PathBuf, PathBuf)> {
    if !(DependencyStatus::check()
        && DependencyStatus::encoder_available("libsvtav1")
        && DependencyStatus::vmaf_available())
    {
        eprintln!("skipping: this FFmpeg cannot encode AV1 and score VMAF");
        return None;
    }
    let dir = std::env::temp_dir().join(format!("av1c_e2e_gate_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(input) = make_video_fixture(&dir) else {
        eprintln!("skipping: could not build the fixture clip");
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    };
    let output = dir.join("out.mkv");
    let mut config = AppConfig {
        encoder: Encoder::SvtAv1,
        ..AppConfig::default()
    };
    config.quality.vmaf_enabled = true;
    config.quality.vmaf_threshold = threshold;
    config.quality.delete_source_on_success = true;

    let identity = crate::queue::SourceIdentity::from_path(&input).unwrap();
    let (hook_input, hook_output) = (input.clone(), output.clone());
    let result = super::run_encoding_pipeline(
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        &identity,
        &metadata_for(&input),
        crate::tracks::OutputTracks::default(),
        DvMode::ToHdr10,
        false,
        Vec::new(),
        &config,
        None,
        cancel,
        Some(Box::new(move || on_before_vmaf(&hook_input, &hook_output))),
    );
    Some((result, input, output))
}

/// Replaces the file at `path` with a byte-identical copy that is a
/// different file.
#[cfg(unix)]
fn replace_with_copy(path: &Path) {
    let copy = path.with_extension("copy");
    std::fs::copy(path, &copy).unwrap();
    std::fs::rename(&copy, path).unwrap();
}

#[test]
fn a_passing_vmaf_score_deletes_the_source() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let Some((result, input, output)) = run_deleting_pipeline("pass", 1.0, &cancel, |_, _| {})
    else {
        return;
    };
    assert!(
        matches!(
            result,
            super::FullEncodeResult::SuccessWithVmaf {
                source_deleted: true,
                ..
            }
        ),
        "{result:?}"
    );
    assert!(!input.exists());
    assert!(output.exists());
    let _ = std::fs::remove_dir_all(input.parent().unwrap());
}

#[test]
fn a_score_below_the_threshold_keeps_the_source() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let Some((result, input, _)) = run_deleting_pipeline("low", 101.0, &cancel, |_, _| {}) else {
        return;
    };
    assert!(
        matches!(result, super::FullEncodeResult::QualityWarning { .. }),
        "{result:?}"
    );
    assert!(input.exists());
    let _ = std::fs::remove_dir_all(input.parent().unwrap());
}

#[test]
fn a_vmaf_run_that_fails_keeps_the_source() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let Some((result, input, _)) = run_deleting_pipeline("fail", 1.0, &cancel, |_, output| {
        std::fs::write(output, b"not a video").unwrap();
    }) else {
        return;
    };
    assert!(
        matches!(result, super::FullEncodeResult::VmafFailed { .. }),
        "{result:?}"
    );
    assert!(input.exists());
    let _ = std::fs::remove_dir_all(input.parent().unwrap());
}

#[test]
fn a_cancel_during_vmaf_keeps_the_source() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&cancel);
    let Some((result, input, _)) = run_deleting_pipeline("cancel", 1.0, &cancel, move |_, _| {
        flag.store(true, std::sync::atomic::Ordering::Release);
    }) else {
        return;
    };
    assert!(
        matches!(result, super::FullEncodeResult::Cancelled),
        "{result:?}"
    );
    assert!(input.exists());
    let _ = std::fs::remove_dir_all(input.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn a_source_replaced_during_the_job_is_kept() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let Some((result, input, _)) =
        run_deleting_pipeline("source", 1.0, &cancel, |input, _| replace_with_copy(input))
    else {
        return;
    };
    assert!(
        matches!(
            result,
            super::FullEncodeResult::SuccessWithVmaf {
                source_deleted: false,
                keep_reason: Some(super::KeepReason::SourceChanged),
                ..
            }
        ),
        "{result:?}"
    );
    assert!(input.exists());
    let _ = std::fs::remove_dir_all(input.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn an_output_replaced_before_deletion_keeps_the_source() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let Some((result, input, _)) = run_deleting_pipeline("output", 1.0, &cancel, |_, output| {
        replace_with_copy(output);
    }) else {
        return;
    };
    assert!(
        matches!(
            result,
            super::FullEncodeResult::SuccessWithVmaf {
                source_deleted: false,
                keep_reason: Some(super::KeepReason::OutputChanged),
                ..
            }
        ),
        "{result:?}"
    );
    assert!(input.exists());
    let _ = std::fs::remove_dir_all(input.parent().unwrap());
}

/// A second video stream or a data stream the output does not carry keeps
/// the source whatever the VMAF score.
#[test]
fn extra_video_or_data_streams_keep_the_source() {
    if !DependencyStatus::check() {
        eprintln!("skipping: ffmpeg is not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!("av1c_e2e_extra_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(single) = make_video_fixture(&dir) else {
        eprintln!("skipping: could not build the fixture clip");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    let two_videos = dir.join("two_videos.mkv");
    let timecode = dir.join("timecode.mov");
    let ffmpeg = |args: &[&str]| {
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    };
    let source = single.to_str().unwrap();
    let built = ffmpeg(&[
        "-i",
        source,
        "-i",
        source,
        "-map",
        "0:v",
        "-map",
        "1:v",
        "-c",
        "copy",
        two_videos.to_str().unwrap(),
    ]) && ffmpeg(&[
        "-i",
        source,
        "-c",
        "copy",
        "-timecode",
        "00:00:00:00",
        timecode.to_str().unwrap(),
    ]);
    if !built {
        eprintln!("skipping: could not build the fixture clips");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    if !probe(&timecode, "stream=codec_type").contains(&"data".to_string()) {
        eprintln!("skipping: this FFmpeg did not write a timecode data stream");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let output = dir.join("out.mkv");
    let reason = |input: &Path| {
        let params = EncodingParams::from_metadata(
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &metadata_for(input),
            &AppConfig::default(),
            crate::tracks::OutputTracks::default(),
            DvMode::ToHdr10,
            false,
            Vec::new(),
        );
        super::keep_source_reason(&params, DvMode::ToHdr10, &AtomicBool::new(false))
    };

    let lost = Some(super::KeepReason::ExtraStreams);
    assert_eq!(reason(&single), None);
    assert_eq!(reason(&two_videos), lost);
    assert_eq!(reason(&timecode), lost);

    let _ = std::fs::remove_dir_all(&dir);
}

/// The TUI encodes with the saved settings: an unsaved "delete source" and
/// threshold left on the Settings screen do not delete the source.
#[test]
fn unsaved_tui_settings_do_not_reach_the_encode() {
    if !(DependencyStatus::check()
        && DependencyStatus::encoder_available("libsvtav1")
        && DependencyStatus::vmaf_available())
    {
        eprintln!("skipping: this FFmpeg cannot encode AV1 and score VMAF");
        return;
    }
    let dir = std::env::temp_dir().join(format!("av1c_e2e_unsaved_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(input) = make_video_fixture(&dir) else {
        eprintln!("skipping: could not build the fixture clip");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };

    let mut app = crate::app::App::new();
    app.saved_config.encoder = Encoder::SvtAv1;
    app.saved_config.quality.vmaf_enabled = true;
    app.saved_config.quality.vmaf_threshold = 1.0;
    app.saved_config.quality.delete_source_on_success = false;
    app.saved_config.disc.staging_directory = Some(dir.to_string_lossy().into_owned());
    app.config = app.saved_config.clone();
    app.config.quality.delete_source_on_success = true;

    let mut job = crate::queue::EncodingJob::new(input.clone());
    job.metadata = Some(metadata_for(&input));
    job.source_identity = Some(crate::queue::SourceIdentity::from_path(&input).unwrap());
    job.output_path = Some(dir.join("out.mkv"));
    job.status = crate::queue::JobStatus::Ready;
    app.queue.jobs.push(job);

    app.start_encoding();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while app.encoding_active && std::time::Instant::now() < deadline {
        app.process_progress_messages();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(!app.encoding_active, "the encode did not finish");
    assert!(
        matches!(
            app.queue.jobs[0].status,
            crate::queue::JobStatus::DoneWithVmaf { .. }
        ),
        "{:?}",
        app.queue.jobs[0].status
    );
    assert!(input.exists(), "an unsaved setting deleted the source");
    let _ = std::fs::remove_dir_all(&dir);
}
