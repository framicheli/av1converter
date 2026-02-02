pub mod encoder_detect;
pub mod types;

pub use encoder_detect::Encoder;
pub use types::*;

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[allow(clippy::derivable_impls)]
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            encoder: Encoder::default(),
            quality: QualityConfig::default(),
            performance: PerformanceConfig::default(),
            presets: EncodingPresetsConfig::default(),
            output: OutputConfig::default(),
            tracks: TrackPresetConfig::default(),
        }
    }
}

impl AppConfig {
    /// Load configuration from TOML file, or create default if not found
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if config_path.exists() {
            match Self::load_from_file(&config_path) {
                Ok(config) => {
                    info!("Loaded config from {}", config_path.display());
                    return config;
                }
                Err(e) => {
                    warn!("Failed to load config: {}. Using defaults.", e);
                }
            }
        }

        let config = Self::default();
        // Save default config for future editing
        if let Err(e) = config.save() {
            warn!("Failed to save default config: {}", e);
        }
        config
    }

    /// Save configuration to TOML file
    pub fn save(&self) -> Result<(), AppError> {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Config(format!("Failed to create config directory: {}", e))
            })?;
        }

        let toml_string = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, toml_string)
            .map_err(|e| AppError::Config(format!("Failed to write config file: {}", e)))?;

        info!("Saved config to {}", config_path.display());
        Ok(())
    }

    /// Load configuration from a specific file
    fn load_from_file(path: &PathBuf) -> Result<Self, AppError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read config file: {}", e)))?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the default configuration file path
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("av1converter")
            .join("config.toml")
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), AppError> {
        if self.quality.vmaf_threshold < 0.0 || self.quality.vmaf_threshold > 100.0 {
            return Err(AppError::Config(
                "VMAF threshold must be between 0 and 100".to_string(),
            ));
        }
        if self.performance.svt_preset > 13 {
            return Err(AppError::Config(
                "SVT-AV1 preset must be between 0 and 13".to_string(),
            ));
        }
        Ok(())
    }

    /// Get the encoding preset for a given resolution tier and HDR status
    pub fn preset_for(
        &self,
        tier: &crate::analyzer::ResolutionTier,
        is_hdr: bool,
    ) -> &EncodingPreset {
        use crate::analyzer::ResolutionTier;
        match (tier, is_hdr) {
            (ResolutionTier::SD, _) => &self.presets.sd,
            (ResolutionTier::HD, _) => &self.presets.hd,
            (ResolutionTier::FullHD, false) => &self.presets.full_hd,
            (ResolutionTier::FullHD, true) => &self.presets.full_hd_hdr,
            (ResolutionTier::Uhd, false) => &self.presets.uhd,
            (ResolutionTier::Uhd, true) | (ResolutionTier::Above4K, true) => &self.presets.uhd_hdr,
            (ResolutionTier::Above4K, false) => &self.presets.uhd,
        }
    }
}
