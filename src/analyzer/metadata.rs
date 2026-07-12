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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DvMode {
    /// Carry the DV RPU into the AV1 stream (DV profile 10, SVT-AV1 only)
    KeepDolbyVision,
    /// Drop the DV layer and produce plain HDR10 output
    #[default]
    ToHdr10,
}

impl DvMode {
    /// Recommended mode for a given DV profile. Profile 5 has no
    /// HDR10-compatible base layer, so tone-mapping to HDR10 is the safer
    /// default; cross-compatible profiles (7/8) keep DV losslessly.
    pub fn recommended_for(dv_profile: Option<u8>) -> Self {
        if dv_profile == Some(5) {
            DvMode::ToHdr10
        } else {
            DvMode::KeepDolbyVision
        }
    }
}

/// HDR10 static metadata (mastering display + content light level)
#[derive(Debug, Clone, Copy, PartialEq)]
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
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub hdr_type: HdrType,
    /// Dolby Vision profile (5, 7, 8, ...) when the source carries DV
    pub dv_profile: Option<u8>,
    /// HDR10 static metadata, when present in the source
    pub hdr10_static: Option<Hdr10StaticMetadata>,
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

    /// Get HDR status string
    pub fn hdr_string(&self) -> &'static str {
        self.hdr_type.display_string()
    }
}
