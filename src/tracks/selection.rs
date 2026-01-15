use super::{AudioTrack, SubtitleTrack};

/// Track selection for encoding
#[derive(Debug, Clone, Default)]
pub struct TrackSelection {
    pub audio_indices: Vec<usize>,
    pub subtitle_indices: Vec<usize>,
}

impl TrackSelection {
    /// Select all available tracks
    pub fn select_all(audio_tracks: &[AudioTrack], subtitle_tracks: &[SubtitleTrack]) -> Self {
        Self {
            audio_indices: audio_tracks.iter().map(|t| t.index).collect(),
            subtitle_indices: subtitle_tracks.iter().map(|t| t.index).collect(),
        }
    }

    /// Toggle an audio track selection
    pub fn toggle_audio(&mut self, index: usize) {
        if self.audio_indices.contains(&index) {
            self.audio_indices.retain(|&i| i != index);
        } else {
            self.audio_indices.push(index);
            self.audio_indices.sort();
        }
    }

    /// Toggle a subtitle track selection
    pub fn toggle_subtitle(&mut self, index: usize) {
        if self.subtitle_indices.contains(&index) {
            self.subtitle_indices.retain(|&i| i != index);
        } else {
            self.subtitle_indices.push(index);
            self.subtitle_indices.sort();
        }
    }
}
