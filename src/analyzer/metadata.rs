/// HDR type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HdrType {
    /// Standard Dynamic Range
    #[default]
    Sdr,
    /// PQ (Perceptual Quantizer) - HDR10/HDR10+
    Pq,
    /// HLG (Hybrid Log-Gamma)
    Hlg,
    /// Dolby Vision
    DolbyVision,
}

impl HdrType {
    /// Check if this is any HDR format
    pub fn is_hdr(&self) -> bool {
        !matches!(self, HdrType::Sdr)
    }

    /// Get display string for this HDR type
    pub fn display_string(&self) -> &'static str {
        match self {
            HdrType::Sdr => "SDR",
            HdrType::Pq => "HDR10",
            HdrType::Hlg => "HLG",
            HdrType::DolbyVision => "Dolby Vision",
        }
    }
}

/// Video metadata from analysis
#[derive(Debug, Clone)]
#[allow(unused)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub hdr_type: HdrType,
    pub codec_name: String,
    pub pixel_format: Option<String>,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub duration_secs: f64,
    pub bitrate: Option<u64>,
}

impl VideoMetadata {
    /// Get resolution string
    pub fn resolution_string(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    /// Get HDR status string
    pub fn hdr_string(&self) -> &'static str {
        self.hdr_type.display_string()
    }
}
