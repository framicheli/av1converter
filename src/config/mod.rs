pub mod encoder_detect;
pub mod types;

pub use encoder_detect::Encoder;
pub use types::*;

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

/// Main application configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Selected encoder
    pub encoder: Encoder,
    /// Quality settings
    pub quality: QualityConfig,
    /// Performance settings
    pub performance: PerformanceConfig,
    /// Encoding presets per resolution tier
    pub presets: EncodingPresetsConfig,
    /// Output settings
    pub output: OutputConfig,
    /// Track selection presets
    pub tracks: TrackPresetConfig,
}

impl AppConfig {
    /// Load configuration from TOML file, or create default if not found.
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if config_path.exists() {
            match Self::load_from_file(&config_path) {
                Ok(mut config) => {
                    config.sanitize();
                    info!("Loaded config from {}", config_path.display());
                    return config;
                }
                Err(e) => {
                    warn!("Failed to load config: {e:?}. Using defaults.");
                }
            }
        }

        let config = Self::default();
        // Save default config for future editing
        if let Err(e) = config.save() {
            warn!("Failed to save default config: {e:?}");
        }
        config
    }

    /// Save configuration to TOML file
    pub fn save(&self) -> Result<(), AppError> {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Config(format!("Failed to create config directory: {e}")))?;
        }

        let toml_string = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, toml_string)
            .map_err(|e| AppError::Config(format!("Failed to write config file: {e}")))?;

        info!("Saved config to {}", config_path.display());
        Ok(())
    }

    /// Load configuration from a specific file
    fn load_from_file(path: &std::path::Path) -> Result<Self, AppError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read config file: {e}")))?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the default configuration file path
    pub fn config_path() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("av1converter")
            .join("config.toml")
    }

    /// Clamp all numeric fields to their valid ranges.
    pub fn sanitize(&mut self) {
        self.quality.vmaf_threshold = self.quality.vmaf_threshold.clamp(0.0, 100.0);
        self.performance.svt_preset = self.performance.svt_preset.min(13);
        for preset in self.presets.all_mut() {
            preset.crf = preset.crf.min(Encoder::SvtAv1.max_quality());
            let hw_max = Encoder::Nvenc.max_quality();
            preset.nvenc_cq = preset.nvenc_cq.min(hw_max);
            preset.qsv_quality = preset.qsv_quality.min(hw_max);
            preset.amf_quality = preset.amf_quality.min(hw_max);
            preset.film_grain = preset.film_grain.min(50);
        }
    }

    /// Get the encoding preset for a given resolution tier and HDR type
    pub fn preset_for(
        &self,
        tier: crate::analyzer::ResolutionTier,
        hdr_type: crate::analyzer::HdrType,
    ) -> &EncodingPreset {
        use crate::analyzer::{HdrType, ResolutionTier};
        match tier {
            ResolutionTier::SD => &self.presets.sd,
            ResolutionTier::HD => &self.presets.hd,
            ResolutionTier::FullHD => match hdr_type {
                HdrType::DolbyVision => &self.presets.full_hd_dv,
                HdrType::Sdr => &self.presets.full_hd,
                _ => &self.presets.full_hd_hdr,
            },
            ResolutionTier::Uhd | ResolutionTier::Above4K => match hdr_type {
                HdrType::DolbyVision => &self.presets.uhd_dv,
                HdrType::Sdr => &self.presets.uhd,
                _ => &self.presets.uhd_hdr,
            },
        }
    }
}
