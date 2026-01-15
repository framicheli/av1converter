/// Resolution tier classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionTier {
    /// SD: up to 720p
    SD,
    /// HD: 720p
    HD,
    /// Full HD: 1080p
    FullHD,
    /// UHD: 4K
    Uhd,
    /// Above 4K
    Above4K,
}

impl ResolutionTier {
    /// Classify resolution into a tier
    pub fn from_dimensions(width: u32, height: u32) -> Self {
        if width >= 4097 || height >= 2161 {
            ResolutionTier::Above4K
        } else if width >= 3000 || height >= 1800 {
            ResolutionTier::Uhd
        } else if width >= 1920 || height >= 721 {
            ResolutionTier::FullHD
        } else if width >= 1280 || height >= 600 {
            ResolutionTier::HD
        } else {
            ResolutionTier::SD
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ResolutionTier::SD => "SD",
            ResolutionTier::HD => "HD 720p",
            ResolutionTier::FullHD => "Full HD 1080p",
            ResolutionTier::Uhd => "4K UHD",
            ResolutionTier::Above4K => "Above 4K",
        }
    }
}

impl std::fmt::Display for ResolutionTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Check if a codec name indicates AV1
pub fn is_av1_codec(codec_name: &str) -> bool {
    let lower = codec_name.to_lowercase();
    lower == "av1" || lower == "av01" || lower == "libaom-av1" || lower == "libsvtav1"
}
