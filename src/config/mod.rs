pub mod encoder_detect;
pub mod types;

pub use encoder_detect::Encoder;
pub use types::*;

use crate::error::AppError;
pub use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

/// Main application configuration
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// UI language
    #[serde(default)]
    pub language: Language,
    /// Selected encoder
    pub encoder: Encoder,
    /// Overall quality preset driving the per-tier encoding presets.
    /// Existing config files (written before this field existed) default to
    /// `Custom` so their hand-tuned presets stay visible and editable.
    #[serde(default = "default_quality_preset")]
    pub quality_preset: QualityPreset,
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
    /// Daemon / web UI settings
    #[serde(default)]
    pub daemon: DaemonConfig,
}

/// Serde default for [`AppConfig::quality_preset`] on legacy config files.
fn default_quality_preset() -> QualityPreset {
    QualityPreset::Custom
}

/// Trim a filename fragment down to characters that are safe inside one path
/// component (no separators, no control characters).
fn strip_path_separators(value: &mut String) {
    *value = value
        .trim()
        .chars()
        .filter(|c| !matches!(c, '/' | '\\') && !c.is_control())
        .collect();
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
                    // Never overwrite a file that failed to parse: a hand-edited
                    // config with one typo would otherwise be silently replaced
                    // by defaults, losing every setting in it.
                    warn!("Failed to load config: {e:?}. Using defaults.");
                    let backup = config_path.with_extension("toml.bak");
                    match std::fs::rename(&config_path, &backup) {
                        Ok(()) => eprintln!(
                            "Could not parse {}: {e}\nIt has been kept as {} and defaults are in use.",
                            config_path.display(),
                            backup.display()
                        ),
                        Err(rename_err) => {
                            warn!("Could not preserve the unreadable config: {rename_err}");
                            eprintln!(
                                "Could not parse {}: {e}\nUsing defaults; the file was left untouched.",
                                config_path.display()
                            );
                            return Self::default();
                        }
                    }
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

    /// Clamp all numeric fields to their valid ranges and repair output naming.
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
        // The suffix and container become part of the output filename, so a
        // stray path separator would write outside the intended directory and
        // an empty suffix would aim the output at the source file itself.
        strip_path_separators(&mut self.output.suffix);
        strip_path_separators(&mut self.output.container);
        self.output.container = self.output.container.trim_matches('.').to_string();
        if self.output.container.is_empty() {
            self.output.container = OutputConfig::default().container;
        }
        // Writing next to the source with no suffix would collide with it.
        if self.output.same_directory && self.output.suffix.is_empty() {
            self.output.suffix = OutputConfig::default().suffix;
        }
        if self.daemon.port == 0 {
            self.daemon.port = DaemonConfig::default().port;
        }
        // An address that cannot be parsed falls back to loopback rather than
        // to a wildcard: a typo must never widen who can reach the daemon.
        if self
            .daemon
            .bind_address
            .parse::<std::net::IpAddr>()
            .is_err()
        {
            self.daemon.bind_address = DaemonConfig::default().bind_address;
        }
        self.daemon.browse_root = self.daemon.browse_root.trim().to_string();
        self.daemon.auth_token = self.daemon.auth_token.trim().to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A config file written before the `language` field existed must still load,
    /// defaulting to English, and the language must round-trip as a string key.
    #[test]
    fn language_defaults_and_round_trips() {
        // Serialize a config without `language` to mimic an older file.
        let mut cfg = AppConfig::default();
        let full = toml::to_string_pretty(&cfg).unwrap();
        let legacy: String = full
            .lines()
            .filter(|l| !l.starts_with("language"))
            .collect::<Vec<_>>()
            .join("\n");
        let loaded: AppConfig = toml::from_str(&legacy).unwrap();
        assert_eq!(loaded.language, Language::English);

        // Chinese serializes to its short code and parses back.
        cfg.language = Language::Chinese;
        let s = toml::to_string_pretty(&cfg).unwrap();
        assert!(s.contains("language = \"zh\""));
        assert_eq!(
            toml::from_str::<AppConfig>(&s).unwrap().language,
            Language::Chinese
        );
    }

    /// A config file written before the `[daemon]` section existed must still
    /// load, with the daemon disabled and default bind address/port.
    #[test]
    fn daemon_defaults_on_legacy_config() {
        let full = toml::to_string_pretty(&AppConfig::default()).unwrap();
        let legacy = full.split("[daemon]").next().unwrap();
        let loaded: AppConfig = toml::from_str(legacy).unwrap();
        assert!(!loaded.daemon.enabled);
        assert_eq!(loaded.daemon, DaemonConfig::default());
    }

    /// An empty suffix next to the source would make the output path collide
    /// with the input file, so `sanitize` restores the default.
    #[test]
    fn empty_suffix_is_repaired_when_writing_next_to_the_source() {
        let mut cfg = AppConfig::default();
        cfg.output.suffix = "   ".to_string();
        cfg.output.same_directory = true;
        cfg.sanitize();
        assert_eq!(cfg.output.suffix, "_av1");

        // Writing to a separate directory has no collision, so "" is kept.
        let mut cfg = AppConfig::default();
        cfg.output.suffix = String::new();
        cfg.output.same_directory = false;
        cfg.sanitize();
        assert_eq!(cfg.output.suffix, "");
    }

    /// Suffix and container are single path components, never paths.
    #[test]
    fn path_separators_are_stripped_from_output_naming() {
        let mut cfg = AppConfig::default();
        cfg.output.suffix = "../../evil".to_string();
        cfg.output.container = "/mkv".to_string();
        cfg.sanitize();
        assert_eq!(cfg.output.suffix, "....evil");
        assert_eq!(cfg.output.container, "mkv");

        let mut cfg = AppConfig::default();
        cfg.output.container = "///".to_string();
        cfg.sanitize();
        assert_eq!(cfg.output.container, "mkv");
    }

    /// Invalid daemon values are repaired by `sanitize`. An unparseable bind
    /// address falls back to loopback, never to a wildcard: a typo must not
    /// widen who can reach the daemon.
    #[test]
    fn daemon_sanitize_repairs_invalid_values() {
        let mut cfg = AppConfig::default();
        cfg.daemon.port = 0;
        cfg.daemon.bind_address = "not-an-ip".to_string();
        cfg.sanitize();
        assert_eq!(cfg.daemon.port, 8399);
        assert_eq!(cfg.daemon.bind_address, "127.0.0.1");
        assert!(!cfg.daemon.binds_publicly());
    }

    /// Only a non-loopback bind address counts as reaching the network.
    #[test]
    fn public_bind_addresses_are_recognised() {
        let mut cfg = AppConfig::default();
        assert!(!cfg.daemon.binds_publicly());
        cfg.daemon.bind_address = "0.0.0.0".to_string();
        assert!(cfg.daemon.binds_publicly());
        cfg.daemon.bind_address = "192.168.1.10".to_string();
        assert!(cfg.daemon.binds_publicly());
        cfg.daemon.bind_address = "::1".to_string();
        assert!(!cfg.daemon.binds_publicly());
    }
}
