use crate::config::{AudioConfig, AudioMode};
use crate::tracks::AudioTrack;

/// Track selection for encoding
#[derive(Debug, Clone, Default)]
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
                AudioStreamPlan {
                    source_index,
                    opus_kbps,
                    independent_mapping: opus_kbps.is_some()
                        && !track
                            .and_then(|t| t.channel_layout.as_deref())
                            .is_some_and(opus_supports_layout),
                }
            })
            .collect();

        OutputTracks {
            audio,
            subtitle_indices: self.subtitle_indices.clone(),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioStreamPlan {
    /// Index among the source's audio streams (`-map 0:a:{source_index}`)
    pub source_index: usize,
    /// `None` copies the stream; `Some(kbps)` re-encodes it to Opus.
    pub opus_kbps: Option<u32>,
    /// Use Opus mapping family 255 for layouts its standard mapping rejects.
    /// This preserves channel count and order instead of downmixing/remapping.
    pub independent_mapping: bool,
}

fn opus_supports_layout(layout: &str) -> bool {
    ["mono", "stereo", "3.0", "quad", "5.0", "5.1", "6.1", "7.1"]
        .iter()
        .any(|supported| layout.eq_ignore_ascii_case(supported))
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

    #[test]
    fn uncommon_opus_layouts_use_independent_mapping() {
        let mut standard = track(0, "aac", Some(6));
        standard.channel_layout = Some("5.1".to_string());
        let mut uncommon = track(1, "aac", Some(3));
        uncommon.channel_layout = Some("2.1".to_string());
        let mut sel = TrackSelection::default();
        sel.set_audio_opus(0, true);
        sel.set_audio_opus(1, true);

        let plan = sel.resolve(&[standard, uncommon], &AudioConfig::default());
        assert!(!plan.audio[0].independent_mapping);
        assert!(plan.audio[1].independent_mapping);
    }
}
