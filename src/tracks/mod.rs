pub mod selection;

/// The selected subtitle tracks, in the order they will be written.
///
/// `subtitle_indices` drives `-map 0:s:N` while [`subtitle_codecs_for`] drives
/// the matching `-c:s:N`, and the two are only in step if both are ordered the
/// same way. `TrackSelection::resolve` sorts the indices; this sorts the tracks
/// to match, so neither side depends on the order ffprobe happened to report
/// the streams in.
pub fn selected_subtitles(tracks: &[SubtitleTrack], indices: &[usize]) -> Vec<SubtitleTrack> {
    let mut selected: Vec<SubtitleTrack> = tracks
        .iter()
        .filter(|track| indices.contains(&track.index))
        .cloned()
        .collect();
    selected.sort_by_key(|track| track.index);
    selected
}

/// The subtitle codec to write for a given output container.
///
/// Text subtitle formats differ between Matroska, `WebM`, and MP4. Convert only
/// the text tracks that the target container cannot hold; bitmap subtitles
/// remain copies so an unsupported combination fails instead of disappearing.
pub fn subtitle_codecs_for(
    output: &std::path::Path,
    selected: &[SubtitleTrack],
) -> Vec<&'static str> {
    let extension = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    selected
        .iter()
        .map(|track| {
            let codec = track.codec.as_str();
            let text = ["ass", "mov_text", "ssa", "srt", "subrip", "text", "webvtt"]
                .iter()
                .any(|candidate| codec.eq_ignore_ascii_case(candidate));
            match extension.to_ascii_lowercase().as_str() {
                "mkv" if codec.eq_ignore_ascii_case("mov_text") => "srt",
                "webm" if text && !codec.eq_ignore_ascii_case("webvtt") => "webvtt",
                "mp4" | "m4v" | "mov" if text && !codec.eq_ignore_ascii_case("mov_text") => {
                    "mov_text"
                }
                _ => "copy",
            }
        })
        .collect()
}

pub use selection::{AudioStreamPlan, OpusLayout, OutputTracks, TrackSelection};

#[cfg(test)]
mod tests {
    use super::{SubtitleTrack, selected_subtitles, subtitle_codecs_for};
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

    fn sub_at(index: usize, codec: &str) -> SubtitleTrack {
        SubtitleTrack {
            index,
            ..sub(codec)
        }
    }

    /// `-map 0:s:N` follows the sorted indices, so the codec list has to as
    /// well — even when ffprobe reported the streams out of order.
    #[test]
    fn selected_subtitles_are_ordered_by_index() {
        let tracks = [sub_at(2, "subrip"), sub_at(0, "hdmv_pgs_subtitle")];
        let selected = selected_subtitles(&tracks, &[0, 2]);

        assert_eq!(
            selected.iter().map(|t| t.index).collect::<Vec<_>>(),
            vec![0, 2]
        );
        // ...which is what puts `copy` on stream 0 and `mov_text` on stream 1.
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &selected),
            ["copy", "mov_text"]
        );
        assert!(selected_subtitles(&tracks, &[]).is_empty());
    }

    #[test]
    fn text_subtitles_are_made_compatible_with_the_container() {
        let mov_text = [sub("mov_text")];
        let subrip = [sub("subrip")];

        assert_eq!(
            subtitle_codecs_for(Path::new("out.mkv"), &mov_text),
            ["srt"]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.MKV"), &mov_text),
            ["srt"]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &mov_text),
            ["copy"]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &subrip),
            ["mov_text"]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.webm"), &subrip),
            ["webvtt"]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.webm"), &[sub("webvtt")]),
            ["copy"]
        );
        assert_eq!(subtitle_codecs_for(Path::new("out.mkv"), &subrip), ["copy"]);
        assert!(subtitle_codecs_for(Path::new("out.mkv"), &[]).is_empty());

        let mixed = [sub("mov_text"), sub("hdmv_pgs_subtitle")];
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mkv"), &mixed),
            ["srt", "copy"]
        );
    }
}

/// Audio track information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub channels: Option<u16>,
    pub channel_layout: Option<String>,
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
