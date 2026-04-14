pub mod selection;

pub use selection::TrackSelection;

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
            .map(|t| format!(" - {}", t))
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
        self.bitrate
            .map(|b| {
                if b >= 1_000_000 {
                    format!("{:.1} Mbps", b as f64 / 1_000_000.0)
                } else {
                    format!("{} kbps", b / 1000)
                }
            })
            .unwrap_or_else(|| "N/A".to_string())
    }

    /// Get sample rate display string
    pub fn sample_rate_string(&self) -> String {
        self.sample_rate
            .map(|s| format!("{:.1} kHz", s as f64 / 1000.0))
            .unwrap_or_else(|| "N/A".to_string())
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
            .map(|t| format!(" - {}", t))
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
