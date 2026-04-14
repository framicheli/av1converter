/// Track selection for encoding
#[derive(Debug, Clone, Default)]
pub struct TrackSelection {
    pub audio_indices: Vec<usize>,
    pub subtitle_indices: Vec<usize>,
}

impl TrackSelection {
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
