/// HDR type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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
    pub fn is_hdr(self) -> bool {
        !matches!(self, HdrType::Sdr)
    }

    /// Get display string for this HDR type
    pub fn display_string(self) -> &'static str {
        match self {
            HdrType::Sdr => "SDR",
            HdrType::Pq => "HDR10",
            HdrType::Hlg => "HLG",
            HdrType::DolbyVision => "Dolby Vision",
        }
    }
}

/// How to handle Dolby Vision sources when re-encoding to AV1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DvMode {
    /// Carry the DV RPU into the AV1 stream (DV profile 10, SVT-AV1 only)
    KeepDolbyVision,
    /// Drop the DV layer and produce plain HDR10 output
    #[default]
    ToHdr10,
}

impl DvMode {
    /// Recommended mode for a given DV profile: HDR10 for profile 5, keep-DV
    /// for the cross-compatible profiles (7/8).
    pub fn recommended_for(dv_profile: Option<u8>) -> Self {
        if dv_profile == Some(5) {
            DvMode::ToHdr10
        } else {
            DvMode::KeepDolbyVision
        }
    }
}

/// HDR10 static metadata (mastering display + content light level)
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hdr10StaticMetadata {
    /// Display primaries as CIE 1931 xy chromaticity coordinates
    pub red: (f64, f64),
    pub green: (f64, f64),
    pub blue: (f64, f64),
    pub white_point: (f64, f64),
    /// Mastering display luminance range in cd/m²
    pub max_luminance: f64,
    pub min_luminance: f64,
    /// Content light level in cd/m² (0 = unknown)
    pub max_cll: u32,
    pub max_fall: u32,
}

impl Hdr10StaticMetadata {
    /// Format as SVT-AV1 `mastering-display` parameter value:
    /// `G(x,y)B(x,y)R(x,y)WP(x,y)L(max,min)`
    pub fn svt_mastering_display(&self) -> String {
        format!(
            "G({:.5},{:.5})B({:.5},{:.5})R({:.5},{:.5})WP({:.5},{:.5})L({:.4},{:.4})",
            self.green.0,
            self.green.1,
            self.blue.0,
            self.blue.1,
            self.red.0,
            self.red.1,
            self.white_point.0,
            self.white_point.1,
            self.max_luminance,
            self.min_luminance,
        )
    }

    /// Format as SVT-AV1 `content-light` parameter value: `max_cll,max_fall`
    pub fn svt_content_light(&self) -> Option<String> {
        if self.max_cll == 0 && self.max_fall == 0 {
            None
        } else {
            Some(format!("{},{}", self.max_cll, self.max_fall))
        }
    }
}

/// Video metadata from analysis
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub hdr_type: HdrType,
    /// Dolby Vision profile (5, 7, 8, ...) when the source carries DV
    pub dv_profile: Option<u8>,
    /// Dolby Vision base-layer compatibility id: 1 = HDR10, 2 = SDR, 4 = HLG
    #[serde(default)]
    pub dv_bl_compat: Option<u8>,
    /// HDR10 static metadata, when present in the source
    pub hdr10_static: Option<Hdr10StaticMetadata>,
    /// HDR10+ dynamic metadata (SMPTE ST 2094-40) found on the first frame
    #[serde(default)]
    pub hdr10_plus: bool,
    pub codec_name: String,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub duration_secs: f64,
}

impl VideoMetadata {
    /// Get resolution string
    pub fn resolution_string(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    /// Get HDR status string, with `HDR10+` added for a source carrying
    /// HDR10+ dynamic metadata
    pub fn hdr_string(&self) -> &'static str {
        match (self.hdr_type, self.hdr10_plus) {
            (HdrType::Pq, true) => "HDR10+",
            (HdrType::DolbyVision, true) => "Dolby Vision + HDR10+",
            (hdr_type, _) => hdr_type.display_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HdrType, VideoMetadata};

    fn metadata(hdr_type: HdrType, hdr10_plus: bool) -> VideoMetadata {
        VideoMetadata {
            width: 3840,
            height: 2160,
            hdr_type,
            dv_profile: None,
            dv_bl_compat: None,
            hdr10_static: None,
            hdr10_plus,
            codec_name: "hevc".to_string(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 60.0,
        }
    }

    #[test]
    fn hdr10_plus_is_named_in_the_hdr_string() {
        assert_eq!(metadata(HdrType::Pq, true).hdr_string(), "HDR10+");
        assert_eq!(metadata(HdrType::Pq, false).hdr_string(), "HDR10");
        assert_eq!(
            metadata(HdrType::DolbyVision, true).hdr_string(),
            "Dolby Vision + HDR10+"
        );
        assert_eq!(
            metadata(HdrType::DolbyVision, false).hdr_string(),
            "Dolby Vision"
        );
    }

    #[test]
    fn metadata_saved_without_the_hdr10_plus_flag_loads_as_not_hdr10_plus() {
        let mut json = serde_json::to_value(metadata(HdrType::Pq, true)).unwrap();
        json.as_object_mut().unwrap().remove("hdr10_plus");
        let loaded: VideoMetadata = serde_json::from_value(json).unwrap();
        assert!(!loaded.hdr10_plus);
        let round_trip: VideoMetadata =
            serde_json::from_value(serde_json::to_value(metadata(HdrType::Pq, true)).unwrap())
                .unwrap();
        assert!(round_trip.hdr10_plus);
    }
}
