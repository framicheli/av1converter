use crate::error::AppError;
use serde::Deserialize;
use serde_json::Value;

/// Video resolutions enum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolution {
    HD1080p,
    HD1080pHDR,
    HD1080pDV, // Don't convert
    UHD2160p,
    UHD2160pHDR,
    UHD2160pDV, // Don't convert
}

#[derive(Debug, Deserialize)]
pub struct AnalysisResult {
    width: u32,
    height: u32,
    pix_fmt: String,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_space: Option<String>,
    side_data_list: Option<Vec<Value>>,
}

impl AnalysisResult {
    pub fn is_hdr(&self, ar: &AnalysisResult) -> bool {
        matches!(
            ar.color_transfer.as_deref(),
            Some("smpte2084") | Some("arib-std-b67")
        )
    }

    pub fn is_dolby_vision(&self, ar: &AnalysisResult) -> bool {
        ar.side_data_list
            .as_ref()
            .map(|list| list.iter().any(|v| v.to_string().contains("Dolby Vision")))
            .unwrap_or(false)
    }

    pub fn classify_video(&self, ar: &AnalysisResult) -> Result<Resolution, AppError> {
        let is_4k = ar.width >= 3840 || ar.height >= 2160;
        let hdr = self.is_hdr(ar);
        let dv = self.is_dolby_vision(ar);

        Ok(match (is_4k, hdr, dv) {
            (false, false, false) => Resolution::HD1080p,
            (false, true, false) => Resolution::HD1080pHDR,
            (false, _, true) => Resolution::HD1080pDV,
            (true, false, false) => Resolution::UHD2160p,
            (true, true, false) => Resolution::UHD2160pHDR,
            (true, _, true) => Resolution::UHD2160pDV,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct AnalysisOutput {
    pub streams: Vec<AnalysisResult>,
}
