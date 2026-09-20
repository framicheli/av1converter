/// Resolution tier classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionTier {
    /// SD: below 720p
    SD,
    /// HD: 720p up to (but not including) 1080p
    HD,
    /// Full HD: 1080p through 1440p (QHD uses the 1080p preset matrix)
    FullHD,
    /// UHD: 4K
    Uhd,
    /// Above 4K
    Above4K,
}

impl ResolutionTier {
    /// Classify resolution into a tier.
    ///
    /// Classifies on the short and long sides: a portrait clip lands in the
    /// same tier as its landscape equivalent (1080×1920 is Full HD, not UHD).
    ///
    /// - Above 4K: long ≥ 4097 or short ≥ 2161
    /// - UHD: long ≥ 3000 or short ≥ 1800
    /// - Full HD: long ≥ 1920 or short ≥ 1080 (includes 1440p)
    /// - HD: long ≥ 1280 or short ≥ 720
    /// - SD: everything else
    pub fn from_dimensions(width: u32, height: u32) -> Self {
        let short = width.min(height);
        let long = width.max(height);
        if long >= 4097 || short >= 2161 {
            ResolutionTier::Above4K
        } else if long >= 3000 || short >= 1800 {
            ResolutionTier::Uhd
        } else if long >= 1920 || short >= 1080 {
            ResolutionTier::FullHD
        } else if long >= 1280 || short >= 720 {
            ResolutionTier::HD
        } else {
            ResolutionTier::SD
        }
    }

    pub fn display_name(self) -> &'static str {
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
        write!(f, "{}", (*self).display_name())
    }
}

/// Check if a codec name indicates AV1
pub fn is_av1_codec(codec_name: &str) -> bool {
    let lower = codec_name.to_lowercase();
    lower == "av1" || lower == "av01" || lower == "libaom-av1" || lower == "libsvtav1"
}

#[cfg(test)]
mod tests {
    use super::ResolutionTier;

    #[test]
    fn classifies_common_boundaries() {
        let cases = [
            ((640, 480), ResolutionTier::SD),
            ((854, 480), ResolutionTier::SD),
            ((1280, 720), ResolutionTier::HD),
            ((1280, 800), ResolutionTier::HD),
            ((1366, 768), ResolutionTier::HD),
            ((1919, 1079), ResolutionTier::HD),
            ((1920, 1080), ResolutionTier::FullHD),
            ((1920, 800), ResolutionTier::FullHD),
            ((2560, 1440), ResolutionTier::FullHD),
            ((2999, 1799), ResolutionTier::FullHD),
            ((3840, 2160), ResolutionTier::Uhd),
            ((3000, 1600), ResolutionTier::Uhd),
            ((4096, 2160), ResolutionTier::Uhd),
            ((4097, 2160), ResolutionTier::Above4K),
            ((3840, 2161), ResolutionTier::Above4K),
            // Portrait 1080p
            ((1080, 1920), ResolutionTier::FullHD),
            ((720, 1280), ResolutionTier::HD),
        ];
        for ((w, h), expected) in cases {
            assert_eq!(ResolutionTier::from_dimensions(w, h), expected, "{w}x{h}");
        }
    }
}
