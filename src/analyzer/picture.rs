//! Black-bar and field-structure detection, and the filter chain that crops
//! and deinterlaces a source before it is encoded.

use crate::config::VideoConfig;
use crate::error::AppError;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tracing::{info, warn};

/// Number of points sampled across the source.
const SAMPLES: u32 = 8;
/// Frames decoded at each sample point.
const FRAMES_PER_SAMPLE: u32 = 48;
/// Time limit for one sample.
const SAMPLE_TIMEOUT: Duration = Duration::from_mins(2);
/// `cropdetect` black threshold, relative to the pixel format's maximum.
const CROP_LIMIT: &str = "0.094";
/// Interlaced share of the determined frames, in percent, at or above which a
/// source is treated as interlaced.
const INTERLACED_PERCENT: u64 = 20;
/// Repeated-field share of the frames, in percent, at or above which an
/// interlaced source is treated as telecined.
const TELECINE_PERCENT: u64 = 15;
/// Determined frames below which the field analysis is inconclusive.
const MIN_DETERMINED_FRAMES: u64 = 30;

/// A crop rectangle in source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
}

/// How the fields of a source are turned into progressive frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldTreatment {
    /// The frames pass through unchanged.
    #[default]
    None,
    /// `bwdif` makes one progressive frame of each interlaced frame.
    Deinterlace { top_field_first: bool },
    /// `fieldmatch` and `decimate` restore the progressive film frames of a
    /// 3:2 telecined source.
    InverseTelecine,
}

/// The picture processing applied to one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PictureFilter {
    pub crop: Option<Crop>,
    pub fields: FieldTreatment,
}

impl PictureFilter {
    /// Whether the filter drops frames, which changes the output timestamps.
    pub fn drops_frames(&self) -> bool {
        self.fields == FieldTreatment::InverseTelecine
    }

    /// The `FFmpeg` filter chain, field processing first and then the crop.
    /// `None` when the filter changes nothing.
    pub fn chain(&self) -> Option<String> {
        let mut filters = Vec::new();
        match self.fields {
            FieldTreatment::None => {}
            FieldTreatment::Deinterlace { top_field_first } => filters.push(format!(
                "bwdif=mode=send_frame:parity={}:deint=all",
                if top_field_first { "tff" } else { "bff" }
            )),
            FieldTreatment::InverseTelecine => filters.push(
                "fieldmatch=order=auto:combmatch=full,\
                 bwdif=mode=send_frame:deint=interlaced,decimate"
                    .to_string(),
            ),
        }
        if let Some(crop) = self.crop {
            filters.push(format!(
                "crop={}:{}:{}:{}",
                crop.width, crop.height, crop.x, crop.y
            ));
        }
        (!filters.is_empty()).then(|| filters.join(","))
    }

    /// A one-line English description for the log.
    pub fn describe(&self) -> String {
        let fields = match self.fields {
            FieldTreatment::None => "no field processing".to_string(),
            FieldTreatment::Deinterlace { top_field_first } => format!(
                "deinterlace ({})",
                if top_field_first { "TFF" } else { "BFF" }
            ),
            FieldTreatment::InverseTelecine => "inverse telecine".to_string(),
        };
        match self.crop {
            Some(crop) => format!(
                "crop to {}x{} at {},{}; {fields}",
                crop.width, crop.height, crop.x, crop.y
            ),
            None => format!("no crop; {fields}"),
        }
    }
}

/// Which detections run for one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PictureRequest {
    pub crop: bool,
    pub deinterlace: bool,
    pub inverse_telecine: bool,
}

impl PictureRequest {
    /// The detections `config` asks for on a job; `disc_rip` is whether the
    /// job's source was ripped from a disc.
    pub fn from_config(config: &VideoConfig, disc_rip: bool) -> Self {
        if config.disc_rips_only && !disc_rip {
            return Self::default();
        }
        Self {
            crop: config.crop,
            deinterlace: config.deinterlace,
            inverse_telecine: config.deinterlace && config.inverse_telecine,
        }
    }

    /// Whether any detection is asked for.
    pub fn any(self) -> bool {
        self.crop || self.deinterlace
    }
}

/// The active picture area one sample saw: the first and last non-black
/// column and row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bounds {
    x1: u32,
    x2: u32,
    y1: u32,
    y2: u32,
}

/// `idet` frame counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct FieldStats {
    tff: u64,
    bff: u64,
    progressive: u64,
    repeated_neither: u64,
    repeated_top: u64,
    repeated_bottom: u64,
}

impl FieldStats {
    fn add(&mut self, other: FieldStats) {
        self.tff += other.tff;
        self.bff += other.bff;
        self.progressive += other.progressive;
        self.repeated_neither += other.repeated_neither;
        self.repeated_top += other.repeated_top;
        self.repeated_bottom += other.repeated_bottom;
    }
}

/// Detect what `request` asks for in the source at `path` and return the
/// filter that undoes it. `field_order` is ffprobe's report for the stream;
/// a `progressive` stream is never deinterlaced.
pub fn detect(
    path: &str,
    width: u32,
    height: u32,
    duration_secs: f64,
    field_order: Option<&str>,
    request: PictureRequest,
    cancel: &AtomicBool,
) -> Result<PictureFilter, AppError> {
    let analyse_fields = request.deinterlace && field_order != Some("progressive");
    if !request.crop && !analyse_fields {
        return Ok(PictureFilter::default());
    }

    let mut filters = Vec::new();
    if request.crop {
        filters.push(format!("cropdetect=limit={CROP_LIMIT}:round=2:reset=0"));
    }
    if analyse_fields {
        filters.push("idet".to_string());
    }
    let filters = filters.join(",");

    let mut bounds = Vec::new();
    let mut stats = FieldStats::default();
    for position in sample_positions(duration_secs) {
        let stderr = run_sample(path, position, &filters, cancel)?;
        if request.crop
            && let Some(sample) = last_bounds(&stderr)
        {
            bounds.push(sample);
        }
        if analyse_fields {
            stats.add(parse_idet(&stderr));
        }
    }

    let filter = PictureFilter {
        crop: if request.crop {
            crop_from_samples(&bounds, width, height)
        } else {
            None
        },
        fields: if analyse_fields {
            classify_fields(stats, request.inverse_telecine)
        } else {
            FieldTreatment::None
        },
    };
    info!("Picture analysis of {path}: {}", filter.describe());
    Ok(filter)
}

/// Seek positions, in seconds, spread evenly across the source and clear of
/// its first and last moments.
fn sample_positions(duration_secs: f64) -> Vec<f64> {
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return vec![0.0];
    }
    (1..=SAMPLES)
        .map(|i| duration_secs * f64::from(i) / f64::from(SAMPLES + 1))
        .collect()
}

/// Decode one sample through `filters` and return the `FFmpeg` log output.
fn run_sample(
    path: &str,
    position: f64,
    filters: &str,
    cancel: &AtomicBool,
) -> Result<String, AppError> {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-ss",
        &format!("{position:.3}"),
        "-i",
        path,
        "-map",
        "0:V:0",
        "-an",
        "-sn",
        "-dn",
        "-frames:v",
        &FRAMES_PER_SAMPLE.to_string(),
        "-vf",
        filters,
        "-f",
        "null",
        "-",
    ]);
    let output = super::ffprobe::run_command(&mut command, cancel, SAMPLE_TIMEOUT)?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        warn!("Picture analysis sample at {position:.1}s failed");
        return Err(AppError::Analysis(format!(
            "Picture analysis failed: {}",
            stderr.lines().last().unwrap_or_default()
        )));
    }
    Ok(stderr)
}

/// The value after `key:` in a `cropdetect` or `idet` log line.
fn field_value(line: &str, key: &str) -> Option<u64> {
    let start = line.find(key)? + key.len();
    line[start..]
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// The bounds in the last `cropdetect` line of `stderr`. A sample that saw
/// only black reports no bounds.
fn last_bounds(stderr: &str) -> Option<Bounds> {
    let line = stderr
        .lines()
        .rev()
        .find(|line| line.contains("cropdetect") && line.contains(" x1:"))?;
    let value = |key| field_value(line, key).and_then(|v| u32::try_from(v).ok());
    let bounds = Bounds {
        x1: value(" x1:")?,
        x2: value(" x2:")?,
        y1: value(" y1:")?,
        y2: value(" y2:")?,
    };
    (bounds.x2 > bounds.x1 && bounds.y2 > bounds.y1).then_some(bounds)
}

/// The crop that keeps every pixel any sample saw as picture, aligned to
/// even offsets and sizes. `None` when fewer than half the samples saw a
/// picture, when the union is the whole frame, or when it would keep less
/// than a quarter of either dimension.
fn crop_from_samples(samples: &[Bounds], width: u32, height: u32) -> Option<Crop> {
    if samples.len() * 2 < SAMPLES as usize || width < 2 || height < 2 {
        return None;
    }
    let x1 = samples.iter().map(|b| b.x1).min()?;
    let x2 = samples.iter().map(|b| b.x2).max()?;
    let y1 = samples.iter().map(|b| b.y1).min()?;
    let y2 = samples.iter().map(|b| b.y2).max()?;

    let even_width = width & !1;
    let even_height = height & !1;
    let x = x1 & !1;
    let y = y1 & !1;
    let right = (x2 + 1).next_multiple_of(2).min(even_width);
    let bottom = (y2 + 1).next_multiple_of(2).min(even_height);
    if right <= x || bottom <= y {
        return None;
    }
    let crop = Crop {
        width: right - x,
        height: bottom - y,
        x,
        y,
    };
    let whole =
        crop.x == 0 && crop.y == 0 && crop.width >= even_width && crop.height >= even_height;
    let too_small = crop.width * 4 < width || crop.height * 4 < height;
    (!whole && !too_small).then_some(crop)
}

/// The `idet` counts in `stderr`: the multi-frame detection and the
/// repeated-field counts.
fn parse_idet(stderr: &str) -> FieldStats {
    let mut stats = FieldStats::default();
    for line in stderr.lines().filter(|line| line.contains("idet")) {
        if line.contains("Multi frame detection:") {
            stats.tff = field_value(line, "TFF:").unwrap_or_default();
            stats.bff = field_value(line, "BFF:").unwrap_or_default();
            stats.progressive = field_value(line, "Progressive:").unwrap_or_default();
        } else if line.contains("Repeated Fields:") {
            stats.repeated_neither = field_value(line, "Neither:").unwrap_or_default();
            stats.repeated_top = field_value(line, "Top:").unwrap_or_default();
            stats.repeated_bottom = field_value(line, "Bottom:").unwrap_or_default();
        }
    }
    stats
}

/// The field treatment the summed `idet` counts call for.
fn classify_fields(stats: FieldStats, inverse_telecine: bool) -> FieldTreatment {
    let interlaced = stats.tff + stats.bff;
    let determined = interlaced + stats.progressive;
    if determined < MIN_DETERMINED_FRAMES || interlaced * 100 < determined * INTERLACED_PERCENT {
        return FieldTreatment::None;
    }
    let repeated = stats.repeated_top + stats.repeated_bottom;
    let frames = repeated + stats.repeated_neither;
    if inverse_telecine && frames > 0 && repeated * 100 >= frames * TELECINE_PERCENT {
        return FieldTreatment::InverseTelecine;
    }
    FieldTreatment::Deinterlace {
        top_field_first: stats.tff >= stats.bff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x1: u32, x2: u32, y1: u32, y2: u32) -> Bounds {
        Bounds { x1, x2, y1, y2 }
    }

    #[test]
    fn the_filter_chain_deinterlaces_before_it_crops() {
        let filter = PictureFilter {
            crop: Some(Crop {
                width: 1920,
                height: 800,
                x: 0,
                y: 140,
            }),
            fields: FieldTreatment::Deinterlace {
                top_field_first: false,
            },
        };
        assert_eq!(
            filter.chain().as_deref(),
            Some("bwdif=mode=send_frame:parity=bff:deint=all,crop=1920:800:0:140")
        );
        assert!(!filter.drops_frames());
    }

    #[test]
    fn inverse_telecine_matches_fields_and_drops_the_repeated_frame() {
        let filter = PictureFilter {
            crop: None,
            fields: FieldTreatment::InverseTelecine,
        };
        assert_eq!(
            filter.chain().as_deref(),
            Some(
                "fieldmatch=order=auto:combmatch=full,\
                 bwdif=mode=send_frame:deint=interlaced,decimate"
            )
        );
        assert!(filter.drops_frames());
    }

    #[test]
    fn an_empty_filter_has_no_chain() {
        let filter = PictureFilter::default();
        assert_eq!(filter.chain(), None);
        assert!(!filter.drops_frames());
    }

    #[test]
    fn disc_rips_only_leaves_other_sources_untouched() {
        let config = VideoConfig {
            crop: true,
            deinterlace: true,
            inverse_telecine: true,
            disc_rips_only: true,
            delete_processed_source: false,
        };
        assert_eq!(
            PictureRequest::from_config(&config, false),
            PictureRequest::default()
        );
        assert_eq!(
            PictureRequest::from_config(&config, true),
            PictureRequest {
                crop: true,
                deinterlace: true,
                inverse_telecine: true,
            }
        );
    }

    #[test]
    fn inverse_telecine_needs_deinterlacing_turned_on() {
        let config = VideoConfig {
            inverse_telecine: true,
            ..VideoConfig::default()
        };
        let request = PictureRequest::from_config(&config, true);
        assert!(!request.inverse_telecine);
        assert!(!request.any());
    }

    #[test]
    fn cropdetect_bounds_come_from_the_last_line() {
        let stderr = "\
[Parsed_cropdetect_0 @ 0x1] x1:0 x2:639 y1:50 y2:300 w:640 h:250 x:0 y:50 pts:1 t:0.04 limit:0.094 crop=640:250:0:50
[Parsed_cropdetect_0 @ 0x1] x1:0 x2:639 y1:44 y2:315 w:640 h:272 x:0 y:44 pts:2542 t:2.542000 limit:0.094000 crop=640:272:0:44
[Parsed_idet_1 @ 0x2] Repeated Fields: Neither:    61 Top:     0 Bottom:     0";
        assert_eq!(last_bounds(stderr), Some(bounds(0, 639, 44, 315)));
    }

    #[test]
    fn an_all_black_sample_reports_no_bounds() {
        let stderr = "[Parsed_cropdetect_0 @ 0x1] x1:639 x2:0 y1:359 y2:0 w:-638 h:-358 x:640 y:360 pts:1 t:0.04 limit:0.094 crop=-638:-358:640:360";
        assert_eq!(last_bounds(stderr), None);
        assert_eq!(last_bounds("no crop lines"), None);
    }

    #[test]
    fn the_crop_keeps_every_pixel_any_sample_saw() {
        let mut samples = vec![bounds(0, 1919, 140, 939); 6];
        samples.push(bounds(0, 1919, 132, 947));
        samples.push(bounds(4, 1915, 141, 938));
        assert_eq!(
            crop_from_samples(&samples, 1920, 1080),
            Some(Crop {
                width: 1920,
                height: 816,
                x: 0,
                y: 132,
            })
        );
    }

    #[test]
    fn crop_offsets_and_sizes_are_even() {
        let samples = vec![bounds(9, 710, 3, 476); 8];
        assert_eq!(
            crop_from_samples(&samples, 720, 480),
            Some(Crop {
                width: 704,
                height: 476,
                x: 8,
                y: 2,
            })
        );
    }

    #[test]
    fn a_full_frame_is_not_cropped() {
        let samples = vec![bounds(0, 1919, 0, 1079); 8];
        assert_eq!(crop_from_samples(&samples, 1920, 1080), None);
    }

    #[test]
    fn too_few_samples_with_a_picture_give_no_crop() {
        let samples = vec![bounds(0, 1919, 140, 939); 3];
        assert_eq!(crop_from_samples(&samples, 1920, 1080), None);
        let samples = vec![bounds(0, 1919, 140, 939); 4];
        assert!(crop_from_samples(&samples, 1920, 1080).is_some());
    }

    #[test]
    fn a_crop_keeping_under_a_quarter_of_the_frame_is_refused() {
        let samples = vec![bounds(0, 1919, 500, 760); 8];
        assert_eq!(crop_from_samples(&samples, 1920, 1080), None);
    }

    #[test]
    fn idet_counts_are_read_from_its_summary() {
        let stderr = "\
[Parsed_idet_1 @ 0x7f] Repeated Fields: Neither:    37 Top:    12 Bottom:    11
[Parsed_idet_1 @ 0x7f] Single frame detection: TFF:    50 BFF:     9 Progressive:     2 Undetermined:     0
[Parsed_idet_1 @ 0x7f] Multi frame detection: TFF:    57 BFF:     4 Progressive:     1 Undetermined:     3";
        assert_eq!(
            parse_idet(stderr),
            FieldStats {
                tff: 57,
                bff: 4,
                progressive: 1,
                repeated_neither: 37,
                repeated_top: 12,
                repeated_bottom: 11,
            }
        );
    }

    fn stats(tff: u64, bff: u64, progressive: u64, repeated: u64, neither: u64) -> FieldStats {
        FieldStats {
            tff,
            bff,
            progressive,
            repeated_neither: neither,
            repeated_top: repeated,
            repeated_bottom: 0,
        }
    }

    #[test]
    fn interlaced_video_is_deinterlaced_with_its_dominant_parity() {
        assert_eq!(
            classify_fields(stats(10, 300, 20, 0, 330), true),
            FieldTreatment::Deinterlace {
                top_field_first: false
            }
        );
        assert_eq!(
            classify_fields(stats(300, 10, 20, 0, 330), false),
            FieldTreatment::Deinterlace {
                top_field_first: true
            }
        );
    }

    #[test]
    fn telecined_film_is_inverse_telecined_only_when_asked() {
        let telecined = stats(150, 10, 220, 150, 230);
        assert_eq!(
            classify_fields(telecined, true),
            FieldTreatment::InverseTelecine
        );
        assert_eq!(
            classify_fields(telecined, false),
            FieldTreatment::Deinterlace {
                top_field_first: true
            }
        );
    }

    #[test]
    fn progressive_video_is_left_alone() {
        assert_eq!(
            classify_fields(stats(10, 5, 370, 0, 384), true),
            FieldTreatment::None
        );
    }

    #[test]
    fn the_interlaced_threshold_is_inclusive() {
        assert_eq!(
            classify_fields(stats(20, 0, 80, 0, 100), false),
            FieldTreatment::Deinterlace {
                top_field_first: true
            }
        );
        assert_eq!(
            classify_fields(stats(19, 0, 81, 0, 100), false),
            FieldTreatment::None
        );
    }

    #[test]
    fn too_few_determined_frames_are_inconclusive() {
        assert_eq!(
            classify_fields(stats(29, 0, 0, 0, 29), false),
            FieldTreatment::None
        );
    }

    #[test]
    fn samples_spread_across_the_source() {
        let positions = sample_positions(90.0);
        assert_eq!(positions.len(), SAMPLES as usize);
        assert!((positions[0] - 10.0).abs() < 1e-9);
        assert!((positions[7] - 80.0).abs() < 1e-9);
        assert_eq!(sample_positions(0.0), vec![0.0]);
        assert_eq!(sample_positions(f64::NAN), vec![0.0]);
    }
}
