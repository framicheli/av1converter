//! Sample-based CRF search: short stretches of the source are encoded at
//! candidate values and scored with VMAF, and the highest value whose samples
//! meet the threshold is kept.

use crate::analyzer::VideoMetadata;
use crate::encoder::command_builder::{EncodingParams, build_sample_args};
use crate::verifier::{self, VmafOutcome, VmafResult, Window};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::info;

static SAMPLE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Length of one sample, in seconds.
const SAMPLE_SECS: f64 = 10.0;
/// Seconds of source per sample taken.
const SECS_PER_SAMPLE: f64 = 600.0;
/// Fewest samples taken from a source longer than three samples.
const MIN_SAMPLES: usize = 2;
/// Most samples taken from one source.
const MAX_SAMPLES: usize = 5;
/// Values searched on each side of the presets-table value.
const SEARCH_SPAN: u8 = 12;
/// VMAF points the samples' mean score must reach above the threshold.
pub const SAMPLE_MARGIN: f64 = 1.0;
/// How far below the searched value the single re-encode goes.
pub const RETRY_STEP: u8 = 4;

/// The value a search settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchResult {
    /// The quality value to encode with.
    pub crf: u8,
    /// Whether the samples met the threshold at `crf`. `false` means no value
    /// in the search range did and `crf` is the lowest one tried.
    pub passed: bool,
}

/// Why a search ended without a result.
#[derive(Debug)]
pub enum SearchError {
    Cancelled,
    Failed(String),
}

/// The stretches of a `duration`-second source that are sampled: the whole
/// source when it is at most three samples long, otherwise one sample per ten
/// minutes, between two and five, spread evenly and centred on their points.
pub fn sample_windows(duration: f64) -> Vec<Window> {
    if !(duration.is_finite() && duration > 0.0) {
        return Vec::new();
    }
    if duration <= SAMPLE_SECS * 3.0 {
        return vec![Window {
            start_secs: 0.0,
            length_secs: duration,
        }];
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = ((duration / SECS_PER_SAMPLE).round() as usize).clamp(MIN_SAMPLES, MAX_SAMPLES);
    #[allow(clippy::cast_precision_loss)]
    (0..count)
        .map(|i| Window {
            start_secs: duration * (i + 1) as f64 / (count + 1) as f64 - SAMPLE_SECS / 2.0,
            length_secs: SAMPLE_SECS,
        })
        .collect()
}

/// The values searched around the presets-table value `start`, within
/// `1..=max`.
pub fn search_range(start: u8, max: u8) -> (u8, u8) {
    let start = start.clamp(1, max.max(1));
    (
        start.saturating_sub(SEARCH_SPAN).max(1),
        start.saturating_add(SEARCH_SPAN).min(max.max(1)),
    )
}

/// Whether sample scores meet `threshold`: their mean by [`SAMPLE_MARGIN`],
/// and their worst frame. No scores never pass.
pub fn samples_pass(scores: &[VmafResult], threshold: f64) -> bool {
    if scores.is_empty() {
        return false;
    }
    #[allow(clippy::cast_precision_loss)]
    let mean = scores.iter().map(|s| s.score).sum::<f64>() / scores.len() as f64;
    let min = scores
        .iter()
        .map(|s| s.min_score)
        .fold(f64::INFINITY, f64::min);
    mean >= threshold + SAMPLE_MARGIN && min >= threshold
}

/// The most probes [`highest_passing`] makes over `lo..=hi` starting at
/// `first`.
fn max_probes(lo: u8, hi: u8, first: u8) -> u32 {
    let first = first.clamp(lo, hi);
    let side = (first - lo).max(hi - first);
    1 + side.bit_width()
}

/// The highest value in `lo..=hi` for which `passes` holds, probing `first`
/// and then bisecting. `None` when no probed value passes.
fn highest_passing<E>(
    lo: u8,
    hi: u8,
    first: u8,
    mut passes: impl FnMut(u8) -> Result<bool, E>,
) -> Result<Option<u8>, E> {
    if lo > hi {
        return Ok(None);
    }
    let (mut lo, mut hi) = (lo, hi);
    let mut best = None;
    let mut probe = first.clamp(lo, hi);
    loop {
        if passes(probe)? {
            best = Some(probe);
            match probe.checked_add(1) {
                Some(next) => lo = next,
                None => break,
            }
        } else {
            match probe.checked_sub(1) {
                Some(next) => hi = next,
                None => break,
            }
        }
        if lo > hi {
            break;
        }
        probe = lo + (hi - lo) / 2;
    }
    Ok(best)
}

/// Search the quality value for `params`, starting from the presets-table
/// value it carries. `on_progress` receives 0–100.
pub fn search(
    params: &EncodingParams,
    metadata: &VideoMetadata,
    threshold: f64,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(f64),
) -> Result<SearchResult, SearchError> {
    let windows = sample_windows(metadata.duration_secs);
    if windows.is_empty() {
        return Err(SearchError::Failed(
            "the source duration is unknown".to_string(),
        ));
    }
    let (lo, hi) = search_range(params.crf, params.encoder.max_quality());
    let total = f64::from(max_probes(lo, hi, params.crf))
        * f64::from(u32::try_from(windows.len()).unwrap_or(u32::MAX));
    let mut done = 0.0;
    on_progress(0.0);

    let found = highest_passing(lo, hi, params.crf, |crf| {
        let mut candidate = params.clone();
        candidate.crf = crf;
        let mut scores = Vec::with_capacity(windows.len());
        for window in &windows {
            scores.push(score_sample(&candidate, metadata, window, cancel)?);
            done += 1.0;
            on_progress((done / total * 100.0).min(99.0));
        }
        let passed = samples_pass(&scores, threshold);
        info!(
            "CRF search: {crf} scored {} on {} sample(s): {}",
            scores
                .iter()
                .map(|s| format!("{:.2}/{:.2}", s.score, s.min_score))
                .collect::<Vec<_>>()
                .join(", "),
            scores.len(),
            if passed { "passes" } else { "fails" }
        );
        Ok(passed)
    })?;
    on_progress(100.0);
    Ok(match found {
        Some(crf) => SearchResult { crf, passed: true },
        None => SearchResult {
            crf: lo,
            passed: false,
        },
    })
}

/// Encode `window` with `params` and score it against the same stretch of the
/// source. The sample file is removed afterwards.
fn score_sample(
    params: &EncodingParams,
    metadata: &VideoMetadata,
    window: &Window,
    cancel: &AtomicBool,
) -> Result<VmafResult, SearchError> {
    let uid = SAMPLE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tag = format!("{}_{uid}", std::process::id());
    let sample = crate::utils::scratch_path(&format!("av1c_sample_{tag}.mkv"))
        .map_err(SearchError::Failed)?;
    let stderr_path = crate::utils::scratch_path(&format!("av1c_sample_stderr_{tag}.txt"))
        .map_err(SearchError::Failed)?;
    let result = encode_sample(params, window, &sample, &stderr_path, cancel).and_then(|()| {
        match verifier::calculate_vmaf(
            std::path::Path::new(&params.input),
            &sample,
            Some(window),
            params.hdr_type,
            metadata.width,
            metadata.height,
            &params.picture,
            cancel,
        ) {
            Ok(VmafOutcome::Scored(score)) => Ok(score),
            Ok(VmafOutcome::Cancelled) => Err(SearchError::Cancelled),
            Err(e) => Err(SearchError::Failed(e.to_string())),
        }
    });
    let _ = std::fs::remove_file(&sample);
    let _ = std::fs::remove_file(&stderr_path);
    result
}

/// Run the sample encode of `window` into `sample`.
fn encode_sample(
    params: &EncodingParams,
    window: &Window,
    sample: &std::path::Path,
    stderr_path: &std::path::Path,
    cancel: &AtomicBool,
) -> Result<(), SearchError> {
    let args = build_sample_args(params, window, &sample.to_string_lossy());
    let stderr_file = std::fs::File::create(stderr_path)
        .map_err(|e| SearchError::Failed(format!("Failed to create sample log file: {e}")))?;
    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file));
    let (mut child, _child) = crate::utils::child::spawn(&mut ffmpeg)
        .map_err(|e| SearchError::Failed(format!("Failed to start ffmpeg: {e}")))?;
    match crate::verifier::vmaf::wait_or_cancel(&mut child, cancel) {
        Ok(Some(status)) if status.success() => Ok(()),
        Ok(Some(_)) => Err(SearchError::Failed(
            crate::encoder::ffmpeg::read_file_tail(stderr_path)
                .map(|tail| tail.lines().last().unwrap_or_default().to_string())
                .unwrap_or_default(),
        )),
        Ok(None) => Err(SearchError::Cancelled),
        Err(e) => Err(SearchError::Failed(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(mean: f64, min: f64) -> VmafResult {
        VmafResult {
            score: mean,
            min_score: min,
            max_score: 100.0,
        }
    }

    #[test]
    fn an_unknown_duration_has_no_samples() {
        assert_eq!(sample_windows(0.0), Vec::new());
        assert_eq!(sample_windows(f64::NAN), Vec::new());
        assert_eq!(sample_windows(-5.0), Vec::new());
    }

    #[test]
    fn a_short_source_is_sampled_whole() {
        assert_eq!(
            sample_windows(30.0),
            vec![Window {
                start_secs: 0.0,
                length_secs: 30.0
            }]
        );
    }

    #[test]
    fn a_source_just_over_three_samples_gets_two_centred_samples() {
        let windows = sample_windows(45.0);
        assert_eq!(
            windows,
            vec![
                Window {
                    start_secs: 10.0,
                    length_secs: 10.0
                },
                Window {
                    start_secs: 25.0,
                    length_secs: 10.0
                },
            ]
        );
    }

    #[test]
    fn a_feature_film_is_sampled_five_times_inside_its_running_time() {
        let duration = 2.0 * 3600.0;
        let windows = sample_windows(duration);
        assert_eq!(windows.len(), MAX_SAMPLES);
        assert!((windows[0].start_secs - (1200.0 - 5.0)).abs() < 1e-9);
        assert!(
            windows
                .iter()
                .all(|w| w.start_secs >= 0.0 && w.start_secs + w.length_secs <= duration)
        );
    }

    #[test]
    fn a_half_hour_episode_gets_three_samples() {
        assert_eq!(sample_windows(1800.0).len(), 3);
    }

    #[test]
    fn the_search_range_stays_inside_the_encoder_scale() {
        assert_eq!(search_range(24, 63), (12, 36));
        assert_eq!(search_range(5, 63), (1, 17));
        assert_eq!(search_range(58, 63), (46, 63));
        assert_eq!(search_range(50, 51), (38, 51));
        assert_eq!(search_range(0, 63), (1, 13));
    }

    #[test]
    fn samples_pass_only_with_the_mean_margin_and_a_passing_minimum() {
        assert!(samples_pass(&[score(91.0, 90.0)], 90.0));
        assert!(!samples_pass(&[score(90.99, 90.0)], 90.0));
        assert!(!samples_pass(&[score(95.0, 89.99)], 90.0));
        // The mean is across samples; one weak sample can sink it.
        assert!(!samples_pass(&[score(94.0, 91.0), score(87.0, 90.5)], 90.0));
        assert!(samples_pass(&[score(94.0, 91.0), score(89.0, 90.5)], 90.0));
        assert!(!samples_pass(&[], 0.0));
    }

    /// The search finds the highest passing value of a monotone quality
    /// curve wherever the threshold lies, and probes no more than it says.
    #[test]
    fn bisection_finds_the_highest_passing_value() {
        for (lo, hi, first) in [(12u8, 36u8, 24u8), (1, 13, 1), (46, 63, 58), (38, 51, 51)] {
            for limit in lo.saturating_sub(1)..=hi {
                let mut probed = Vec::new();
                let found = highest_passing::<()>(lo, hi, first, |crf| {
                    probed.push(crf);
                    Ok(crf <= limit)
                })
                .unwrap();
                let expected = (limit >= lo).then_some(limit.min(hi));
                assert_eq!(found, expected, "range {lo}..={hi}, limit {limit}");
                assert_eq!(probed[0], first);
                let mut unique = probed.clone();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(unique.len(), probed.len(), "no value is probed twice");
                assert!(
                    u32::try_from(probed.len()).unwrap() <= max_probes(lo, hi, first),
                    "{probed:?} exceeds {}",
                    max_probes(lo, hi, first)
                );
            }
        }
    }

    #[test]
    fn a_probe_error_ends_the_search() {
        let result = highest_passing(
            12,
            36,
            24,
            |crf| if crf == 24 { Ok(true) } else { Err(crf) },
        );
        assert_eq!(result, Err(30));
    }
}
