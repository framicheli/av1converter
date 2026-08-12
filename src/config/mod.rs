pub mod encoder_detect;
pub mod types;

pub use encoder_detect::Encoder;
pub use types::*;

use crate::error::AppError;
pub use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::io::Write;
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
    /// Audio transcoding settings
    #[serde(default)]
    pub audio: AudioConfig,
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
                    eprintln!(
                        "Could not parse {}: {e}\nUsing defaults; the file was left untouched.",
                        config_path.display()
                    );
                    return Self::default();
                }
            }
        }

        let config = Self {
            encoder: encoder_detect::detect_encoder(),
            ..Self::default()
        };
        if let Err(e) = config.save() {
            warn!("Failed to save default config: {e:?}");
        }
        config
    }

    /// Save configuration to TOML file
    pub fn save(&self) -> Result<(), AppError> {
        let config_path = Self::config_path();
        self.save_to_path(&config_path)?;
        info!("Saved config to {}", config_path.display());
        Ok(())
    }

    fn save_to_path(&self, config_path: &std::path::Path) -> Result<(), AppError> {
        if let Some(parent) = config_path.parent() {
            crate::utils::ensure_private_dir(parent)
                .map_err(|e| AppError::Config(format!("Failed to create config directory: {e}")))?;
        }

        Self::preserve_unreadable(config_path)?;

        let toml_string = toml::to_string_pretty(self)?;
        let nonce = crate::utils::random_hex(8)
            .map_err(|e| AppError::Config(format!("Failed to create config temp file: {e}")))?;
        let tmp = config_path.with_extension(format!("toml.{nonce}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|e| AppError::Config(format!("Failed to open config file: {e}")))?;
        let result = file
            .write_all(toml_string.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&tmp, config_path));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|e| AppError::Config(format!("Failed to write config file: {e}")))
    }

    /// Copy `path` aside if it exists but cannot be parsed.
    fn preserve_unreadable(path: &std::path::Path) -> Result<(), AppError> {
        if !path.exists() || Self::load_from_file(path).is_ok() {
            return Ok(());
        }
        let backup = path.with_extension("toml.bak");
        let contents = std::fs::read(path).map_err(|e| {
            AppError::Config(format!("Could not read {} for backup: {e}", path.display()))
        })?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&backup).map_err(|e| {
            AppError::Config(format!(
                "Refusing to overwrite the unreadable config at {}: could not create backup {} ({e})",
                path.display(),
                backup.display()
            ))
        })?;
        if let Err(e) = file.write_all(&contents) {
            let _ = std::fs::remove_file(&backup);
            return Err(AppError::Config(format!(
                "Could not write backup {}: {e}",
                backup.display()
            )));
        }
        warn!("Kept the unreadable config as {}", backup.display());
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
        if !PerformanceConfig::valid_nvenc_preset(&self.performance.nvenc_preset) {
            self.performance.nvenc_preset = PerformanceConfig::default().nvenc_preset;
        }
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
        // Opus bitrate is multiplied by the channel count, so an absurd
        // per-channel value would ask libopus for a rate it rejects outright.
        self.audio.opus_bitrate_per_channel = self
            .audio
            .opus_bitrate_per_channel
            .clamp(AudioConfig::MIN_PER_CHANNEL, AudioConfig::MAX_PER_CHANNEL);
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

    #[test]
    fn config_save_round_trips_privately_without_leaving_a_temp_file() {
        let dir = std::env::temp_dir().join(format!("av1c_cfg_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        let cfg = AppConfig {
            language: Language::Italian,
            ..AppConfig::default()
        };

        cfg.save_to_path(&path).unwrap();

        assert_eq!(AppConfig::load_from_file(&path).unwrap(), cfg);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
                0
            );
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
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

    /// A config file written before the `[audio]` section existed must still
    /// load, with audio copied exactly as it was before the feature landed.
    #[test]
    fn audio_defaults_on_legacy_config() {
        let full = toml::to_string_pretty(&AppConfig::default()).unwrap();
        let legacy = full.split("[audio]").next().unwrap();
        let loaded: AppConfig = toml::from_str(legacy).unwrap();
        assert_eq!(loaded.audio, AudioConfig::default());
        assert_eq!(loaded.audio.default_mode, AudioMode::Copy);
    }

    /// A config with a typo in it is kept.
    #[test]
    fn an_unreadable_config_is_preserved_before_it_is_overwritten() {
        let dir = std::env::temp_dir().join(format!("av1c_cfg_backup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let backup = dir.join("config.toml.bak");

        // A file that parses needs no copy.
        std::fs::write(
            &path,
            toml::to_string_pretty(&AppConfig::default()).unwrap(),
        )
        .unwrap();
        AppConfig::preserve_unreadable(&path).unwrap();
        assert!(!backup.exists());

        // One that does not is kept verbatim.
        std::fs::write(&path, b"this is not = valid toml [[[").unwrap();
        AppConfig::preserve_unreadable(&path).unwrap();
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            b"this is not = valid toml [[["
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backup).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        std::fs::write(&path, b"another invalid config [[[").unwrap();
        assert!(AppConfig::preserve_unreadable(&path).is_err());
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            b"this is not = valid toml [[["
        );

        // A missing file is simply nothing to protect.
        std::fs::remove_file(&path).unwrap();
        AppConfig::preserve_unreadable(&path).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A per-channel bitrate outside libopus' useful range is clamped, not
    /// passed through to be multiplied by the channel count.
    #[test]
    fn opus_bitrate_per_channel_is_clamped() {
        let mut cfg = AppConfig::default();
        cfg.audio.opus_bitrate_per_channel = 9000;
        cfg.sanitize();
        assert_eq!(
            cfg.audio.opus_bitrate_per_channel,
            AudioConfig::MAX_PER_CHANNEL
        );

        cfg.audio.opus_bitrate_per_channel = 0;
        cfg.sanitize();
        assert_eq!(
            cfg.audio.opus_bitrate_per_channel,
            AudioConfig::MIN_PER_CHANNEL
        );
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

    #[test]
    fn invalid_nvenc_preset_falls_back_to_the_supported_default() {
        let mut cfg = AppConfig::default();
        cfg.performance.nvenc_preset = "slowest".to_string();
        cfg.sanitize();
        assert_eq!(cfg.performance.nvenc_preset, "p4");
    }

    /// The printed URL has to be a link the user can actually open, which a
    /// wildcard bind address is not.
    #[test]
    fn wildcard_binds_are_shown_as_loopback() {
        let mut cfg = DaemonConfig::default();
        assert_eq!(cfg.url(), "http://127.0.0.1:8399/");

        cfg.bind_address = "0.0.0.0".to_string();
        assert_eq!(cfg.url(), "http://127.0.0.1:8399/");
        cfg.bind_address = "::".to_string();
        assert_eq!(cfg.url(), "http://[::1]:8399/");

        // A real address is kept, and IPv6 stays bracketed.
        cfg.bind_address = "192.168.1.10".to_string();
        assert_eq!(cfg.url(), "http://192.168.1.10:8399/");
        cfg.bind_address = "::1".to_string();
        assert_eq!(cfg.url(), "http://[::1]:8399/");

        cfg.auth_token = "abc".to_string();
        assert_eq!(cfg.url(), "http://[::1]:8399/#token=abc");
        cfg.auth_token = "a&b #+%".to_string();
        assert_eq!(cfg.url(), "http://[::1]:8399/#token=a%26b%20%23%2B%25");
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
        assert_eq!(cfg.daemon.listen_address(), "[::1]:8399");
    }
}
