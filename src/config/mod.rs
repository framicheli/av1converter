pub mod encoder_detect;
pub mod settings;
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
    /// Overall quality preset, default to
    /// `Custom`.
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
    /// Disc ripping settings
    #[serde(default)]
    pub disc: DiscConfig,
}

/// Serde default for [`AppConfig::quality_preset`] on legacy config files.
fn default_quality_preset() -> QualityPreset {
    QualityPreset::Custom
}

/// Replace a leading `~` or `~/` with the home directory.
fn expand_home(path: &mut String) {
    let rest = match path.as_str() {
        "~" => "",
        p if p.starts_with("~/") => &p[1..],
        _ => return,
    };
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|home| !home.is_empty())
    {
        *path = format!("{}{rest}", home.to_string_lossy());
    }
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
                    // A file that failed to parse is left untouched, not
                    // overwritten with defaults.
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
        // `clamp` passes NaN through; a non-finite threshold takes the default.
        if !self.quality.vmaf_threshold.is_finite() {
            self.quality.vmaf_threshold = QualityConfig::default().vmaf_threshold;
        }
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
        // The suffix and container become part of the output filename, so path
        // separators are stripped out of both.
        strip_path_separators(&mut self.output.suffix);
        strip_path_separators(&mut self.output.container);
        self.output.container = self.output.container.trim_matches('.').to_string();
        if self.output.container.is_empty() {
            self.output.container = OutputConfig::default().container;
        }
        // Next to the source, an empty suffix collides with the input.
        if self.output.same_directory && self.output.suffix.is_empty() {
            self.output.suffix = OutputConfig::default().suffix;
        }
        if self.daemon.port == 0 {
            self.daemon.port = DaemonConfig::default().port;
        }
        // An unparseable address falls back to loopback, never a wildcard.
        if self
            .daemon
            .bind_address
            .parse::<std::net::IpAddr>()
            .is_err()
        {
            self.daemon.bind_address = DaemonConfig::default().bind_address;
        }
        // Multiplied by the channel count, so it is clamped to a rate libopus
        // accepts.
        self.audio.opus_bitrate_per_channel = self
            .audio
            .opus_bitrate_per_channel
            .clamp(AudioConfig::MIN_PER_CHANNEL, AudioConfig::MAX_PER_CHANNEL);
        self.daemon.browse_root = self.daemon.browse_root.trim().to_string();
        self.daemon.auth_token = self.daemon.auth_token.trim().to_string();
        if let Some(directory) = self.output.output_directory.as_mut() {
            expand_home(directory);
        }
    }

    /// Validate values that settings interfaces accept from users.
    pub fn validate_settings(&self) -> Result<(), String> {
        if !self.quality.vmaf_threshold.is_finite()
            || !(0.0..=100.0).contains(&self.quality.vmaf_threshold)
        {
            return Err("VMAF threshold must be between 0 and 100".to_string());
        }
        if self.performance.svt_preset > 13 {
            return Err("SVT preset must be between 0 and 13".to_string());
        }
        if !PerformanceConfig::valid_nvenc_preset(&self.performance.nvenc_preset) {
            return Err("NVENC preset must be between p1 and p7".to_string());
        }
        if !(AudioConfig::MIN_PER_CHANNEL..=AudioConfig::MAX_PER_CHANNEL)
            .contains(&self.audio.opus_bitrate_per_channel)
        {
            return Err("Opus bitrate per channel must be between 16 and 256".to_string());
        }
        let presets: [&EncodingPreset; 8] = [
            &self.presets.sd,
            &self.presets.hd,
            &self.presets.full_hd,
            &self.presets.full_hd_hdr,
            &self.presets.full_hd_dv,
            &self.presets.uhd,
            &self.presets.uhd_hdr,
            &self.presets.uhd_dv,
        ];
        if presets.iter().any(|preset| {
            preset.crf > Encoder::SvtAv1.max_quality()
                || preset.nvenc_cq > Encoder::Nvenc.max_quality()
                || preset.qsv_quality > Encoder::Qsv.max_quality()
                || preset.amf_quality > Encoder::Amf.max_quality()
                || preset.film_grain > 50
        }) {
            return Err("one or more quality or film-grain values are out of range".to_string());
        }
        if self.daemon.port == 0 {
            return Err("daemon port must be between 1 and 65535".to_string());
        }
        if self
            .daemon
            .bind_address
            .parse::<std::net::IpAddr>()
            .is_err()
        {
            return Err("daemon bind address must be an IP address".to_string());
        }
        Ok(())
    }

    /// Validate changed host paths and store their resolved forms.
    pub fn normalize_changed_host_paths(&mut self, previous: &Self) -> Result<(), String> {
        if self.daemon.browse_root != previous.daemon.browse_root
            && !self.daemon.browse_root.is_empty()
        {
            let root = std::path::Path::new(&self.daemon.browse_root)
                .canonicalize()
                .map_err(|_| "daemon browse root must be an existing directory".to_string())?;
            if !root.is_dir() {
                return Err("daemon browse root must be an existing directory".to_string());
            }
            self.daemon.browse_root = root.to_string_lossy().into_owned();
        }
        if self.disc.staging_directory != previous.disc.staging_directory
            && let Some(directory) = &self.disc.staging_directory
        {
            let directory = std::path::Path::new(directory)
                .canonicalize()
                .map_err(|_| "disc staging directory must exist".to_string())?;
            if !directory.is_dir() {
                return Err("disc staging directory must be a directory".to_string());
            }
            self.disc.staging_directory = Some(directory.to_string_lossy().into_owned());
        }
        if self.disc.makemkvcon_path != previous.disc.makemkvcon_path
            && let Some(executable) = &self.disc.makemkvcon_path
        {
            let executable = std::path::Path::new(executable)
                .canonicalize()
                .map_err(|_| "MakeMKV executable must exist".to_string())?;
            if !executable.is_file() {
                return Err("MakeMKV executable must be a file".to_string());
            }
            self.disc.makemkvcon_path = Some(executable.to_string_lossy().into_owned());
        }
        Ok(())
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

    #[test]
    fn a_tilde_output_directory_expands_to_the_home_directory() {
        let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
            return;
        };
        let home = home.to_string_lossy().into_owned();
        let mut cfg = AppConfig::default();
        cfg.output.output_directory = Some("~/Videos".to_string());
        cfg.sanitize();
        assert_eq!(
            cfg.output.output_directory.as_deref(),
            Some(format!("{home}/Videos").as_str())
        );

        cfg.output.output_directory = Some("~".to_string());
        cfg.sanitize();
        assert_eq!(cfg.output.output_directory.as_deref(), Some(home.as_str()));

        cfg.output.output_directory = Some("/data/~tilde".to_string());
        cfg.sanitize();
        assert_eq!(cfg.output.output_directory.as_deref(), Some("/data/~tilde"));
    }

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

    /// `sanitize` restores the default suffix when writing next to the source.
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

    /// `sanitize` repairs invalid daemon values, falling back to loopback and
    /// never to a wildcard.
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

    #[test]
    fn changed_host_paths_must_resolve_to_their_expected_types() {
        let previous = AppConfig::default();
        let mut cfg = previous.clone();
        cfg.daemon.browse_root = std::env::temp_dir().to_string_lossy().into_owned();
        cfg.disc.staging_directory = Some(std::env::temp_dir().to_string_lossy().into_owned());
        cfg.normalize_changed_host_paths(&previous).unwrap();
        assert!(std::path::Path::new(&cfg.daemon.browse_root).is_absolute());

        let mut invalid = previous.clone();
        invalid.disc.makemkvcon_path = Some("/path/that/does/not/exist".to_string());
        assert!(invalid.normalize_changed_host_paths(&previous).is_err());
    }

    #[test]
    fn unchanged_unavailable_host_paths_do_not_block_other_settings() {
        let previous = AppConfig {
            disc: DiscConfig {
                makemkvcon_path: Some("/temporarily/unavailable/makemkvcon".to_string()),
                ..DiscConfig::default()
            },
            ..AppConfig::default()
        };
        let mut cfg = previous.clone();
        cfg.output.suffix = "_new".to_string();
        cfg.normalize_changed_host_paths(&previous).unwrap();
    }
}
