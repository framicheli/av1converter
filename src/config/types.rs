use serde::{Deserialize, Serialize};

/// Overall quality preset that drives the per-tier encoding settings.
///
/// `Low`, `Medium` and `High` apply a fixed set of per-resolution values,
/// while `Custom` leaves the user's hand-tuned values untouched and editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum QualityPreset {
    #[serde(rename = "low")]
    Low,
    #[default]
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "custom")]
    Custom,
}

impl QualityPreset {
    /// All presets, in display/cycle order.
    pub const ALL: [QualityPreset; 4] = [
        QualityPreset::Low,
        QualityPreset::Medium,
        QualityPreset::High,
        QualityPreset::Custom,
    ];

    /// The next preset in [`QualityPreset::ALL`], wrapping around.
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The previous preset in [`QualityPreset::ALL`], wrapping around.
    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// The per-tier encoding presets for this quality level, or `None` for
    /// [`QualityPreset::Custom`] (which keeps the user's own values).
    pub fn presets(self) -> Option<EncodingPresetsConfig> {
        match self {
            QualityPreset::Low => Some(EncodingPresetsConfig::low()),
            QualityPreset::Medium => Some(EncodingPresetsConfig::medium()),
            QualityPreset::High => Some(EncodingPresetsConfig::high()),
            QualityPreset::Custom => None,
        }
    }
}

/// Quality configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityConfig {
    /// VMAF quality threshold (0-100)
    pub vmaf_threshold: f64,
    /// Whether to run VMAF after encoding
    pub vmaf_enabled: bool,
    /// Delete source file after encoding when VMAF score meets threshold
    #[serde(default)]
    pub delete_source_on_success: bool,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            vmaf_threshold: 90.0,
            vmaf_enabled: true,
            delete_source_on_success: false,
        }
    }
}

/// Performance configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            nvenc_preset: "p4".to_string(),
        }
    }
}

/// Encoding preset for a specific resolution tier
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl EncodingPreset {
    /// Shift every quality value by `delta` (a higher value means lower quality
    /// and smaller files). Film grain synthesis is left untouched.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn shifted(&self, delta: i16) -> EncodingPreset {
        // clamp(0, 63) keeps the result well within u8 range before the cast.
        let adj = |v: u8| -> u8 { (i16::from(v) + delta).clamp(0, 63) as u8 };
        EncodingPreset {
            crf: adj(self.crf),
            film_grain: self.film_grain,
            nvenc_cq: adj(self.nvenc_cq),
            qsv_quality: adj(self.qsv_quality),
            amf_quality: adj(self.amf_quality),
        }
    }
}

/// Encoding presets per resolution tier
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl EncodingPresetsConfig {
    /// Balanced presets — identical to the built-in defaults.
    pub fn medium() -> Self {
        Self::default()
    }

    /// Smaller-file presets (higher CRF/CQ across every tier).
    pub fn low() -> Self {
        Self::default().shifted(4)
    }

    /// Higher-quality presets (lower CRF/CQ across every tier).
    pub fn high() -> Self {
        Self::default().shifted(-4)
    }

    /// Apply a quality shift to every resolution tier.
    fn shifted(self, delta: i16) -> Self {
        Self {
            sd: self.sd.shifted(delta),
            hd: self.hd.shifted(delta),
            full_hd: self.full_hd.shifted(delta),
            full_hd_hdr: self.full_hd_hdr.shifted(delta),
            full_hd_dv: self.full_hd_dv.shifted(delta),
            uhd: self.uhd.shifted(delta),
            uhd_hdr: self.uhd_hdr.shifted(delta),
            uhd_dv: self.uhd_dv.shifted(delta),
        }
    }

    pub fn all_mut(&mut self) -> [&mut EncodingPreset; 8] {
        [
            &mut self.sd,
            &mut self.hd,
            &mut self.full_hd,
            &mut self.full_hd_hdr,
            &mut self.full_hd_dv,
            &mut self.uhd,
            &mut self.uhd_hdr,
            &mut self.uhd_dv,
        ]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output file suffix
    pub suffix: String,
    /// Output container format
    pub container: String,
    /// Whether to place output in same directory as source
    pub same_directory: bool,
    /// Custom output directory (if `same_directory` is false)
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

/// Daemon / web UI configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Whether `--daemon` is allowed to start
    pub enabled: bool,
    /// Bind address for the web server. Defaults to loopback: the web UI can
    /// queue encodes and delete sources, so reaching the network is opt-in.
    pub bind_address: String,
    /// TCP port for the web server
    pub port: u16,
    /// Directory the web file browser is confined to. Empty means the whole
    /// filesystem, which is only reasonable while bound to loopback.
    #[serde(default)]
    pub browse_root: String,
    /// Shared secret required by the `/api` endpoints. Empty disables the
    /// check; set it whenever the daemon is reachable from the network.
    #[serde(default)]
    pub auth_token: String,
}

impl DaemonConfig {
    /// Bind addresses that expose the daemon beyond this machine.
    pub fn binds_publicly(&self) -> bool {
        match self.bind_address.parse::<std::net::IpAddr>() {
            Ok(ip) => !ip.is_loopback(),
            Err(_) => false,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_address: "127.0.0.1".to_string(),
            port: 8399,
            browse_root: String::new(),
            auth_token: String::new(),
        }
    }
}

/// Track selection preset configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
