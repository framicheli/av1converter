pub mod selection;

/// The selected subtitle tracks, sorted by index — the order they will be
/// written. `TrackSelection::resolve` sorts the indices the same way, so the
/// `-map 0:s:N` and `-c:s:N` lists line up whatever order ffprobe reported.
pub fn selected_subtitles(tracks: &[SubtitleTrack], indices: &[usize]) -> Vec<SubtitleTrack> {
    let mut selected: Vec<SubtitleTrack> = tracks
        .iter()
        .filter(|track| indices.contains(&track.index))
        .cloned()
        .collect();
    selected.sort_by_key(|track| track.index);
    selected
}

/// The subtitle codec to write for each selected track in a given output
/// container, or `None` for a track the container cannot hold.
///
/// Matroska holds every format except teletext and CEA-608 captions, which are
/// left out; `mov_text` is converted to SRT there. `WebM`
/// holds only `WebVTT`, and MP4 only `mov_text` and DVD bitmaps: other text
/// tracks are converted, other bitmap tracks are left out.
pub fn subtitle_codecs_for(
    output: &std::path::Path,
    selected: &[SubtitleTrack],
) -> Vec<Option<&'static str>> {
    let extension = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    selected
        .iter()
        .map(|track| {
            let is = |name: &str| track.codec.eq_ignore_ascii_case(name);
            let text = ["ass", "mov_text", "ssa", "srt", "subrip", "text", "webvtt"]
                .into_iter()
                .any(is);
            match extension.to_ascii_lowercase().as_str() {
                "mkv" if is("mov_text") => Some("srt"),
                "mkv" if is("dvb_teletext") || is("eia_608") => None,
                "webm" if is("webvtt") => Some("copy"),
                "webm" if text => Some("webvtt"),
                "mp4" | "m4v" | "mov" if is("mov_text") || is("dvd_subtitle") => Some("copy"),
                "mp4" | "m4v" | "mov" if text => Some("mov_text"),
                "webm" | "mp4" | "m4v" | "mov" => None,
                _ => Some("copy"),
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
        // ...which is what leaves out stream 0 and puts `mov_text` on stream 1.
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &selected),
            [None, Some("mov_text")]
        );
        assert!(selected_subtitles(&tracks, &[]).is_empty());
    }

    #[test]
    fn teletext_and_cea608_are_left_out_of_matroska() {
        assert_eq!(
            subtitle_codecs_for(
                Path::new("x.mkv"),
                &[
                    sub("dvb_teletext"),
                    sub("eia_608"),
                    sub("hdmv_pgs_subtitle")
                ]
            ),
            [None, None, Some("copy")]
        );
    }

    #[test]
    fn text_subtitles_are_made_compatible_with_the_container() {
        let mov_text = [sub("mov_text")];
        let subrip = [sub("subrip")];

        assert_eq!(
            subtitle_codecs_for(Path::new("out.mkv"), &mov_text),
            [Some("srt")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.MKV"), &mov_text),
            [Some("srt")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &mov_text),
            [Some("copy")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &subrip),
            [Some("mov_text")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.webm"), &subrip),
            [Some("webvtt")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.webm"), &[sub("webvtt")]),
            [Some("copy")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mkv"), &subrip),
            [Some("copy")]
        );
        assert!(subtitle_codecs_for(Path::new("out.mkv"), &[]).is_empty());

        let mixed = [sub("mov_text"), sub("hdmv_pgs_subtitle")];
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mkv"), &mixed),
            [Some("srt"), Some("copy")]
        );
    }

    /// Bitmap subtitles are left out of a container that cannot hold them.
    #[test]
    fn unsupported_bitmap_subtitles_are_left_out() {
        let bitmaps = [sub("hdmv_pgs_subtitle"), sub("dvd_subtitle")];
        assert_eq!(
            subtitle_codecs_for(Path::new("out.webm"), &bitmaps),
            [None, None]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mp4"), &bitmaps),
            [None, Some("copy")]
        );
        assert_eq!(
            subtitle_codecs_for(Path::new("out.mkv"), &bitmaps),
            [Some("copy"), Some("copy")]
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
