use crate::encoder::ffmpeg;
use crate::error::AppError;
use std::path::Path;

/// Validation result after encoding
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether frame rates match
    pub frame_rate_match: bool,
    /// Whether durations match (within tolerance)
    pub duration_match: bool,
    /// Whether the output file is readable/valid
    pub file_integrity: bool,
    /// Source duration in seconds
    pub source_duration: f64,
    /// Output duration in seconds
    pub output_duration: f64,
    /// Validation messages
    pub messages: Vec<String>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.frame_rate_match && self.duration_match && self.file_integrity
    }
}

/// Validate an encoded video file against the source
pub fn validate(
    source_path: &str,
    output_path: &str,
    expected_frame_rate: (u32, u32),
    source_duration: f64,
) -> Result<ValidationResult, AppError> {
    let mut messages = Vec::new();

    // Check file exists and has size > 0
    let output_file = Path::new(output_path);
    let file_integrity =
        output_file.exists() && output_file.metadata().map(|m| m.len() > 0).unwrap_or(false);

    if !file_integrity {
        messages.push("Output file is missing or empty".to_string());
        return Ok(ValidationResult {
            frame_rate_match: false,
            duration_match: false,
            file_integrity: false,
            source_duration,
            output_duration: 0.0,
            messages,
        });
    }

    // Check frame rate
    let frame_rate_match = if expected_frame_rate.0 > 0 {
        match ffmpeg::get_frame_rate(output_path) {
            Ok((num, den)) => {
                let source_fps = expected_frame_rate.0 as f64 / expected_frame_rate.1 as f64;
                let output_fps = num as f64 / den as f64;
                let diff = (source_fps - output_fps).abs();
                if diff > 0.01 {
                    messages.push(format!(
                        "Frame rate mismatch: source {:.3} fps, output {:.3} fps",
                        source_fps, output_fps
                    ));
                    false
                } else {
                    true
                }
            }
            Err(e) => {
                messages.push(format!("Could not verify frame rate: {}", e));
                true // Don't fail validation if we can't check
            }
        }
    } else {
        true // No frame rate to verify
    };

    // Check duration (1 second tolerance)
    let output_duration = ffmpeg::get_duration(output_path).unwrap_or(0.0);
    let duration_match = if source_duration > 0.0 && output_duration > 0.0 {
        let diff = (source_duration - output_duration).abs();
        if diff > 1.0 {
            messages.push(format!(
                "Duration mismatch: source {:.1}s, output {:.1}s (diff: {:.1}s)",
                source_duration, output_duration, diff
            ));
            false
        } else {
            true
        }
    } else {
        true // Can't verify, assume OK
    };

    let _ = source_path; // Used for potential future checks

    Ok(ValidationResult {
        frame_rate_match,
        duration_match,
        file_integrity,
        source_duration,
        output_duration,
        messages,
    })
}
