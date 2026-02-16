use serde::{Deserialize, Serialize};

/// Quality configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityConfig {
    /// VMAF quality threshold (0-100)
    pub vmaf_threshold: f64,
    /// Whether to run VMAF after encoding
    pub vmaf_enabled: bool,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            vmaf_threshold: 90.0,
            vmaf_enabled: true,
        }
    }
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// SVT-AV1 preset (0-13, lower = slower/better)
    pub svt_preset: u8,
    /// NVENC preset name
    pub nvenc_preset: String,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            svt_preset: 4,
            nvenc_preset: "p7".to_string(),
        }
    }
}

/// Encoding preset for a specific resolution tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingPreset {
    /// CRF value for software encoding
    pub crf: u8,
    /// Film grain synthesis level (0-50)
    pub film_grain: u8,
    /// CQ value for NVENC
    pub nvenc_cq: u8,
    /// Quality value for QSV
    pub qsv_quality: u8,
    /// Quality value for AMF
    pub amf_quality: u8,
}

/// Encoding presets per resolution tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingPresetsConfig {
    pub sd: EncodingPreset,
    pub hd: EncodingPreset,
    pub full_hd: EncodingPreset,
    pub full_hd_hdr: EncodingPreset,
    #[serde(default = "default_full_hd_dv")]
    pub full_hd_dv: EncodingPreset,
    pub uhd: EncodingPreset,
    pub uhd_hdr: EncodingPreset,
    #[serde(default = "default_uhd_dv")]
    pub uhd_dv: EncodingPreset,
}

fn default_full_hd_dv() -> EncodingPreset {
    EncodingPreset {
        crf: 20,
        film_grain: 3,
        nvenc_cq: 21,
        qsv_quality: 20,
        amf_quality: 21,
    }
}

fn default_uhd_dv() -> EncodingPreset {
    EncodingPreset {
        crf: 20,
        film_grain: 4,
        nvenc_cq: 20,
        qsv_quality: 20,
        amf_quality: 20,
    }
}

impl Default for EncodingPresetsConfig {
    fn default() -> Self {
        Self {
            sd: EncodingPreset {
                crf: 24,
                film_grain: 0,
                nvenc_cq: 26,
                qsv_quality: 24,
                amf_quality: 26,
            },
            hd: EncodingPreset {
                crf: 23,
                film_grain: 0,
                nvenc_cq: 25,
                qsv_quality: 23,
                amf_quality: 25,
            },
            full_hd: EncodingPreset {
                crf: 22,
                film_grain: 0,
                nvenc_cq: 24,
                qsv_quality: 22,
                amf_quality: 24,
            },
            full_hd_hdr: EncodingPreset {
                crf: 23,
                film_grain: 3,
                nvenc_cq: 23,
                qsv_quality: 23,
                amf_quality: 23,
            },
            full_hd_dv: default_full_hd_dv(),
            uhd: EncodingPreset {
                crf: 23,
                film_grain: 4,
                nvenc_cq: 25,
                qsv_quality: 24,
                amf_quality: 25,
            },
            uhd_hdr: EncodingPreset {
                crf: 22,
                film_grain: 4,
                nvenc_cq: 22,
                qsv_quality: 22,
                amf_quality: 22,
            },
            uhd_dv: default_uhd_dv(),
        }
    }
}

/// Output configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output file suffix
    pub suffix: String,
    /// Output container format
    pub container: String,
    /// Whether to place output in same directory as source
    pub same_directory: bool,
    /// Custom output directory (if same_directory is false)
    pub output_directory: Option<String>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            suffix: "_av1".to_string(),
            container: "mkv".to_string(),
            same_directory: true,
            output_directory: None,
        }
    }
}

/// Track selection preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPresetConfig {
    /// Preferred audio languages
    pub preferred_audio_languages: Vec<String>,
    /// Preferred subtitle languages
    pub preferred_subtitle_languages: Vec<String>,
    /// Whether to auto-select all tracks when no preference matches
    pub select_all_fallback: bool,
}

impl Default for TrackPresetConfig {
    fn default() -> Self {
        Self {
            preferred_audio_languages: vec!["eng".to_string(), "ita".to_string()],
            preferred_subtitle_languages: vec!["eng".to_string()],
            select_all_fallback: true,
        }
    }
}
