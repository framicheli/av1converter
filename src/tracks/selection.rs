use crate::config::{AudioConfig, AudioMode};
use crate::tracks::AudioTrack;

/// Track selection for encoding
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TrackSelection {
    pub audio_indices: Vec<usize>,
    pub subtitle_indices: Vec<usize>,
    /// Audio tracks to re-encode to Opus. Always a subset of `audio_indices`:
    /// a stale entry here would shift every per-stream codec option onto the
    /// wrong output stream.
    pub audio_to_opus: Vec<usize>,
}

impl TrackSelection {
    /// Toggle an audio track selection
    pub fn toggle_audio(&mut self, index: usize) {
        if self.audio_indices.contains(&index) {
            self.audio_indices.retain(|&i| i != index);
            // Dropping the track drops its transcode flag with it.
            self.audio_to_opus.retain(|&i| i != index);
        } else {
            self.audio_indices.push(index);
            self.audio_indices.sort_unstable();
        }
    }

    /// Toggle a subtitle track selection
    pub fn toggle_subtitle(&mut self, index: usize) {
        if self.subtitle_indices.contains(&index) {
            self.subtitle_indices.retain(|&i| i != index);
        } else {
            self.subtitle_indices.push(index);
            self.subtitle_indices.sort_unstable();
        }
    }

    /// Whether this audio track is marked for Opus transcoding
    pub fn is_opus(&self, index: usize) -> bool {
        self.audio_to_opus.contains(&index)
    }

    /// Turn Opus transcoding on or off for one audio track. Turning it on also
    /// selects the track, so the subset invariant cannot be broken from the UI.
    pub fn set_audio_opus(&mut self, index: usize, opus: bool) {
        if opus {
            if !self.audio_indices.contains(&index) {
                self.audio_indices.push(index);
                self.audio_indices.sort_unstable();
            }
            if !self.audio_to_opus.contains(&index) {
                self.audio_to_opus.push(index);
                self.audio_to_opus.sort_unstable();
            }
        } else {
            self.audio_to_opus.retain(|&i| i != index);
        }
    }

    /// Cycle one audio track between copy and Opus, selecting it if needed.
    pub fn toggle_audio_opus(&mut self, index: usize) {
        let opus = self.is_opus(index);
        self.set_audio_opus(index, !opus);
    }

    /// Resolve the selection against the source's audio tracks into the exact
    /// stream list the encoder should write.
    pub fn resolve(&self, audio_tracks: &[AudioTrack], config: &AudioConfig) -> OutputTracks {
        let audio = self
            .audio_indices
            .iter()
            .map(|&source_index| {
                let track = audio_tracks.iter().find(|t| t.index == source_index);
                let already_opus = track.is_some_and(|t| {
                    t.codec.eq_ignore_ascii_case("opus") && config.skip_already_opus
                });
                let opus_kbps = if self.is_opus(source_index) && !already_opus {
                    Some(config.opus_bitrate_kbps(track.and_then(|t| t.channels)))
                } else {
                    None
                };
                let layout = if opus_kbps.is_none() {
                    OpusLayout::AsIs
                } else {
                    opus_layout(track.and_then(|t| t.channel_layout.as_deref()))
                };
                AudioStreamPlan {
                    source_index,
                    opus_kbps,
                    layout,
                    title: match (opus_kbps, track) {
                        (Some(_), Some(t)) => retitle(t, layout),
                        _ => None,
                    },
                }
            })
            .collect();

        // Sorted here so the `-map 0:s:N` order and the codec list that
        // `subtitle_codecs_for` produces are both in index order, whatever
        // order the selection was built up in.
        let mut subtitle_indices = self.subtitle_indices.clone();
        subtitle_indices.sort_unstable();

        OutputTracks {
            audio,
            subtitle_indices,
        }
    }

    /// Apply the configured default to every selected audio track. Used when a
    /// job is auto-configured, where nobody picks tracks by hand.
    pub fn apply_audio_default(&mut self, config: &AudioConfig) {
        self.audio_to_opus = match config.default_mode {
            AudioMode::Copy => Vec::new(),
            AudioMode::Opus => self.audio_indices.clone(),
        };
    }
}

/// One audio stream in the output, in output order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioStreamPlan {
    /// Index among the source's audio streams (`-map 0:a:{source_index}`)
    pub source_index: usize,
    /// `None` copies the stream; `Some(kbps)` re-encodes it to Opus.
    pub opus_kbps: Option<u32>,
    /// How the source's channel layout is handed to libopus.
    pub layout: OpusLayout,
    /// Track title to write, or `None` to keep the source's.
    pub title: Option<String>,
}

/// How a transcoded stream's channel layout reaches libopus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpusLayout {
    /// Copied stream, or a layout libopus already accepts.
    #[default]
    AsIs,
    /// Relabel to this standard layout name before encoding.
    Relabel(&'static str),
    /// Mapping family 255: every channel kept, no channel positions declared.
    Independent,
}

/// Codec names as they turn up in track titles, lowercased.
const CODEC_NAMES: [&str; 14] = [
    "e-ac-3", "e-ac3", "eac3", "ac-3", "ac3", "dd+", "ddp", "dts", "truehd", "atmos", "aac",
    "flac", "mp3", "pcm",
];

/// The title for a transcoded stream: `Opus <layout>` when the source's title
/// names a codec, `None` when it describes the content instead.
fn retitle(track: &AudioTrack, layout: OpusLayout) -> Option<String> {
    let title = track.title.as_deref()?.to_ascii_lowercase();
    if !CODEC_NAMES.iter().any(|codec| title.contains(codec)) {
        return None;
    }
    // The layout the output declares, which is not always the source's.
    Some(match (layout, track.channel_layout.as_deref()) {
        (OpusLayout::Relabel(relabelled), _) => format!("Opus {relabelled}"),
        (_, Some(source)) => format!("Opus {source}"),
        (_, None) => "Opus".to_string(),
    })
}

/// How to hand this source layout to libopus.
fn opus_layout(layout: Option<&str>) -> OpusLayout {
    match layout {
        Some(l) if opus_supports_layout(l) => OpusLayout::AsIs,
        Some(l) => standard_spelling(l).map_or(OpusLayout::Independent, OpusLayout::Relabel),
        None => OpusLayout::Independent,
    }
}

/// Whether Opus' standard channel mapping covers this layout. These are the
/// Vorbis layouts; libopus rejects any other spelling.
///
/// ```text
/// $ ffmpeg -i 5.1side.mkv -c:a libopus out.mkv
/// [libopus] Invalid channel layout 5.1(side) for specified mapping family -1.
/// ```
fn opus_supports_layout(layout: &str) -> bool {
    ["mono", "stereo", "3.0", "quad", "5.0", "5.1", "6.1", "7.1"]
        .iter()
        .any(|supported| layout.eq_ignore_ascii_case(supported))
}

/// The standard name for a layout that differs from one only in spelling: the
/// same channels in the same order, so `aformat` relabels without touching a
/// sample. Layouts that differ by more than the name (`7.1(wide)` carries
/// front-of-centre channels where 7.1 carries sides) are not listed — those
/// would rematrix, and keep mapping family 255 instead.
fn standard_spelling(layout: &str) -> Option<&'static str> {
    [
        ("5.0(side)", "5.0"),
        ("5.1(side)", "5.1"),
        ("6.1(back)", "6.1"),
    ]
    .iter()
    .find(|(spelling, _)| layout.eq_ignore_ascii_case(spelling))
    .map(|&(_, standard)| standard)
}

/// The audio and subtitle streams to write, already resolved from the user's
/// selection. The encoder never sees [`TrackSelection`]: a plan whose order
/// matches the output stream order is what per-stream codec options need.
#[derive(Debug, Clone, Default)]
pub struct OutputTracks {
    pub audio: Vec<AudioStreamPlan>,
    pub subtitle_indices: Vec<usize>,
}

impl OutputTracks {
    /// Whether any audio stream is re-encoded rather than copied.
    pub fn transcodes_audio(&self) -> bool {
        self.audio.iter().any(|a| a.opus_kbps.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(index: usize, codec: &str, channels: Option<u16>) -> AudioTrack {
        AudioTrack {
            index,
            language: None,
            codec: codec.to_string(),
            channels,
            channel_layout: None,
            title: None,
            bitrate: None,
            sample_rate: None,
        }
    }

    /// Deselecting a track must clear its Opus flag: a leftover index would
    /// place `-c:a:N libopus` on whatever stream ended up in that slot.
    #[test]
    fn deselecting_a_track_clears_its_opus_flag() {
        let mut sel = TrackSelection::default();
        sel.toggle_audio(0);
        sel.toggle_audio(1);
        sel.set_audio_opus(1, true);
        assert!(sel.is_opus(1));

        sel.toggle_audio(1);
        assert!(!sel.is_opus(1));
        assert!(sel.audio_to_opus.is_empty());
    }

    /// Marking a track for Opus selects it, so the subset invariant holds even
    /// when the user presses the transcode key on an unselected row.
    #[test]
    fn marking_opus_selects_the_track() {
        let mut sel = TrackSelection::default();
        sel.set_audio_opus(2, true);
        assert_eq!(sel.audio_indices, vec![2]);
        assert_eq!(sel.audio_to_opus, vec![2]);
    }

    /// Bitrate follows the source channel count; the layout is never changed.
    #[test]
    fn opus_bitrate_scales_with_channel_count() {
        let tracks = [
            track(0, "aac", Some(2)),
            track(1, "dts", Some(6)),
            track(2, "truehd", Some(8)),
            track(3, "ac3", None),
        ];
        let config = AudioConfig::default();
        let mut sel = TrackSelection::default();
        for i in 0..4 {
            sel.set_audio_opus(i, true);
        }

        let plan = sel.resolve(&tracks, &config);
        assert_eq!(plan.audio[0].opus_kbps, Some(128));
        assert_eq!(plan.audio[1].opus_kbps, Some(384));
        assert_eq!(plan.audio[2].opus_kbps, Some(512));
        // Unknown channel count falls back to stereo rather than guessing high.
        assert_eq!(plan.audio[3].opus_kbps, Some(128));
    }

    /// Re-encoding Opus to Opus is pure generation loss, so it is skipped.
    #[test]
    fn already_opus_tracks_are_copied() {
        let tracks = [track(0, "Opus", Some(6))];
        let mut sel = TrackSelection::default();
        sel.set_audio_opus(0, true);

        let config = AudioConfig::default();
        assert_eq!(sel.resolve(&tracks, &config).audio[0].opus_kbps, None);

        let forced = AudioConfig {
            skip_already_opus: false,
            ..AudioConfig::default()
        };
        assert_eq!(sel.resolve(&tracks, &forced).audio[0].opus_kbps, Some(384));
    }

    /// Subtitle indices come out sorted whatever order they went in, so they
    /// line up with the codec list built from the tracks.
    #[test]
    fn resolved_subtitle_indices_are_sorted() {
        let sel = TrackSelection {
            subtitle_indices: vec![3, 0, 2],
            ..TrackSelection::default()
        };
        assert_eq!(
            sel.resolve(&[], &AudioConfig::default()).subtitle_indices,
            vec![0, 2, 3]
        );
    }

    /// The resolved plan is in output order, which is what `-c:a:N` indexes.
    #[test]
    fn plan_follows_the_mapped_stream_order() {
        let tracks = [
            track(0, "aac", Some(2)),
            track(1, "ac3", Some(6)),
            track(2, "aac", Some(2)),
        ];
        let mut sel = TrackSelection::default();
        sel.toggle_audio(2);
        sel.toggle_audio(0);
        sel.set_audio_opus(2, true);

        let plan = sel.resolve(&tracks, &AudioConfig::default());
        assert_eq!(plan.audio[0].source_index, 0);
        assert_eq!(plan.audio[0].opus_kbps, None);
        assert_eq!(plan.audio[1].source_index, 2);
        assert_eq!(plan.audio[1].opus_kbps, Some(128));
        assert!(plan.transcodes_audio());
    }

    fn layout_of(layout: &str) -> OpusLayout {
        let mut source = track(0, "eac3", Some(6));
        source.channel_layout = Some(layout.to_string());
        let mut sel = TrackSelection::default();
        sel.set_audio_opus(0, true);
        sel.resolve(&[source], &AudioConfig::default()).audio[0].layout
    }

    #[test]
    fn standard_layouts_are_encoded_as_they_are() {
        assert_eq!(layout_of("5.1"), OpusLayout::AsIs);
        assert_eq!(layout_of("stereo"), OpusLayout::AsIs);
    }

    /// The spellings ffprobe reports for real AC-3/E-AC-3/DTS surround tracks
    /// are relabelled, not left to mapping family 255.
    #[test]
    fn qualified_surround_spellings_are_relabelled_not_left_unmapped() {
        assert_eq!(layout_of("5.1(side)"), OpusLayout::Relabel("5.1"));
        assert_eq!(layout_of("5.0(side)"), OpusLayout::Relabel("5.0"));
        assert_eq!(layout_of("6.1(back)"), OpusLayout::Relabel("6.1"));
    }

    /// Layouts a rename cannot reach keep independent streams.
    #[test]
    fn layouts_that_differ_by_more_than_a_name_stay_independent() {
        for layout in [
            "7.1(wide)",
            "7.1(wide-side)",
            "6.1(front)",
            "2.1",
            "22.2",
            "hexadecagonal",
        ] {
            assert_eq!(layout_of(layout), OpusLayout::Independent, "{layout}");
        }
    }

    fn titled(source_title: &str, layout: &str) -> Option<String> {
        let mut source = track(0, "eac3", Some(6));
        source.title = Some(source_title.to_string());
        source.channel_layout = Some(layout.to_string());
        let mut sel = TrackSelection::default();
        sel.set_audio_opus(0, true);
        sel.resolve(&[source], &AudioConfig::default()).audio[0]
            .title
            .clone()
    }

    /// A title naming the source codec becomes `Opus <output layout>`.
    #[test]
    fn titles_naming_the_old_codec_are_rewritten() {
        assert_eq!(
            titled("E-AC3 5.1 @ 640 kbps", "5.1(side)"),
            Some("Opus 5.1".to_string())
        );
        assert_eq!(
            titled("English DD+ 5.1", "5.1(side)"),
            Some("Opus 5.1".to_string())
        );
        assert_eq!(titled("DTS-HD MA 7.1", "7.1"), Some("Opus 7.1".to_string()));
    }

    /// A title describing the content, not the encoding, is kept.
    #[test]
    fn descriptive_titles_are_left_alone() {
        assert_eq!(titled("Director's commentary", "5.1(side)"), None);
        assert_eq!(titled("English", "stereo"), None);
    }

    /// A copied stream keeps the title it came with.
    #[test]
    fn copied_streams_keep_their_title() {
        let mut source = track(0, "eac3", Some(6));
        source.title = Some("E-AC3 5.1".to_string());
        let sel = TrackSelection {
            audio_indices: vec![0],
            ..TrackSelection::default()
        };
        assert_eq!(
            sel.resolve(&[source], &AudioConfig::default()).audio[0].title,
            None
        );
    }

    /// A copied stream is never filtered or remapped, whatever its layout.
    #[test]
    fn copied_streams_keep_their_layout_untouched() {
        let mut source = track(0, "eac3", Some(6));
        source.channel_layout = Some("5.1(side)".to_string());
        let sel = TrackSelection {
            audio_indices: vec![0],
            ..TrackSelection::default()
        };
        let plan = sel.resolve(&[source], &AudioConfig::default());
        assert_eq!(plan.audio[0].layout, OpusLayout::AsIs);
    }
}
