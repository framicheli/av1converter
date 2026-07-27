pub mod selection;

/// The subtitle codec to write for a given output container.
///
/// `mov_text` is MP4's own text format and Matroska has no place for it, so
/// copying it into an `.mkv` fails the entire encode. Converting to `SubRip`
/// keeps the track instead of losing the job.
pub fn subtitle_codec_for(output: &std::path::Path, selected: &[SubtitleTrack]) -> &'static str {
    let matroska = output
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mkv") || e.eq_ignore_ascii_case("webm"));
    let has_mov_text = selected
        .iter()
        .any(|t| t.codec.eq_ignore_ascii_case("mov_text"));

    if matroska && has_mov_text {
        "srt"
    } else {
        "copy"
    }
}

pub use selection::TrackSelection;

#[cfg(test)]
mod tests {
    use super::{SubtitleTrack, subtitle_codec_for};
    use std::path::Path;

    fn sub(codec: &str) -> SubtitleTrack {
        SubtitleTrack {
            index: 0,
            language: None,
            codec: codec.to_string(),
            title: None,
            forced: false,
        }
    }

    #[test]
    fn mov_text_is_converted_only_when_matroska_cannot_hold_it() {
        let mov_text = [sub("mov_text")];
        let subrip = [sub("subrip")];

        assert_eq!(subtitle_codec_for(Path::new("out.mkv"), &mov_text), "srt");
        assert_eq!(subtitle_codec_for(Path::new("out.MKV"), &mov_text), "srt");
        // MP4 keeps its own format, and other codecs are copied as they are.
        assert_eq!(subtitle_codec_for(Path::new("out.mp4"), &mov_text), "copy");
        assert_eq!(subtitle_codec_for(Path::new("out.mkv"), &subrip), "copy");
        assert_eq!(subtitle_codec_for(Path::new("out.mkv"), &[]), "copy");
    }
}

/// Audio track information
#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub channels: Option<u16>,
    pub title: Option<String>,
    pub bitrate: Option<u64>,
    pub sample_rate: Option<u32>,
}

impl AudioTrack {
    pub fn display_name(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let title = self
            .title
            .as_ref()
            .map(|t| format!(" - {t}"))
            .unwrap_or_default();
        let channels_str = match self.channels {
            Some(1) => "Mono",
            Some(2) => "Stereo",
            Some(6) => "5.1",
            Some(8) => "7.1",
            Some(_) => "Multi",
            None => "Unknown",
        };
        format!(
            "{}: {} ({} {}){}",
            self.index,
            lang,
            self.codec.to_uppercase(),
            channels_str,
            title
        )
    }

    /// Get bitrate display string
    pub fn bitrate_string(&self) -> String {
        self.bitrate.map_or_else(
            || "N/A".to_string(),
            |b| {
                if b >= 1_000_000 {
                    // Convert to kbps first (fits in u32 for any real-world bitrate)
                    let kbps = u32::try_from(b / 1000).unwrap_or(u32::MAX);
                    format!("{:.1} Mbps", f64::from(kbps) / 1000.0)
                } else {
                    format!("{} kbps", b / 1000)
                }
            },
        )
    }

    /// Get sample rate display string
    pub fn sample_rate_string(&self) -> String {
        self.sample_rate.map_or_else(
            || "N/A".to_string(),
            |s| format!("{:.1} kHz", f64::from(s) / 1000.0),
        )
    }
}

/// Subtitle track information
#[derive(Debug, Clone)]
pub struct SubtitleTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub title: Option<String>,
    pub forced: bool,
}

impl SubtitleTrack {
    pub fn display_name(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let title = self
            .title
            .as_ref()
            .map(|t| format!(" - {t}"))
            .unwrap_or_default();
        let forced_str = if self.forced { " [Forced]" } else { "" };
        format!(
            "{}: {} ({}){}{}",
            self.index,
            lang,
            self.codec.to_uppercase(),
            forced_str,
            title
        )
    }
}
