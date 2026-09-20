use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// AV1 encoders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoder {
    /// NVIDIA NVENC
    Nvenc,
    /// Intel Quick Sync Video (Arc GPUs)
    Qsv,
    /// AMD AMF
    Amf,
    /// SVT-AV1 software encoder
    SvtAv1,
}

impl Encoder {
    /// `FFmpeg` encoder name
    pub fn ffmpeg_name(self) -> &'static str {
        match self {
            Encoder::Nvenc => "av1_nvenc",
            Encoder::Qsv => "av1_qsv",
            Encoder::Amf => "av1_amf",
            Encoder::SvtAv1 => "libsvtav1",
        }
    }

    /// Display name for UI
    pub fn display_name(self) -> &'static str {
        match self {
            Encoder::Nvenc => "NVENC (NVIDIA)",
            Encoder::Qsv => "Quick Sync (Intel)",
            Encoder::Amf => "AMF (AMD)",
            Encoder::SvtAv1 => "SVT-AV1 (Software)",
        }
    }

    /// Display name in the active language. Only the SVT-AV1 label carries a
    /// translated word; the others are brand names.
    pub fn display_name_in(self, lang: crate::i18n::Language) -> String {
        match self {
            Encoder::SvtAv1 => crate::i18n::t(lang, crate::i18n::Msg::EncoderSvtAv1).to_string(),
            other => other.display_name().to_string(),
        }
    }

    /// Maximum RF/CRF quality value for this encoder
    pub const fn max_quality(self) -> u8 {
        match self {
            Self::SvtAv1 => 63,
            Self::Nvenc | Self::Qsv | Self::Amf => 51,
        }
    }
}

/// Software encoding
impl Default for Encoder {
    fn default() -> Self {
        Encoder::SvtAv1
    }
}

impl std::fmt::Display for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", (*self).display_name())
    }
}

/// Longest a test encode may run before its encoder counts as unavailable.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Detect available AV1 encoder
///
/// Priority: the first hardware encoder that encodes a test frame, else SVT-AV1.
pub fn detect_encoder() -> Encoder {
    [Encoder::Nvenc, Encoder::Qsv, Encoder::Amf]
        .into_iter()
        .find(|encoder| encodes_a_frame(encoder.ffmpeg_name()))
        .unwrap_or(Encoder::SvtAv1)
}

/// Whether `FFmpeg` encodes one frame with the named encoder on this machine
/// within [`PROBE_TIMEOUT`].
fn encodes_a_frame(name: &str) -> bool {
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=black:size=256x256:duration=0.1",
            "-frames:v",
            "1",
            "-c:v",
            name,
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok((mut child, _child)) = crate::utils::child::spawn(&mut command) else {
        return false;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                crate::utils::child::kill_and_wait(&mut child);
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::encodes_a_frame;

    #[test]
    fn an_encoder_ffmpeg_cannot_run_is_not_detected() {
        assert!(!encodes_a_frame("av1_no_such_encoder"));
    }
}
