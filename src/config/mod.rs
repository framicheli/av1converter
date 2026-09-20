pub mod encoder_detect;
pub mod settings;
pub mod types;

pub use encoder_detect::Encoder;
pub use types::*;

use crate::error::AppError;
pub use crate::i18n::Language;
use crate::i18n::{Msg, t};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use tracing::{info, warn};

/// Main application configuration
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// UI language
    #[serde(default)]
    pub language: Language,
    /// Selected encoder
    pub encoder: Encoder,
    /// Overall quality preset. New configs and `Default` use `Medium`; a
    /// missing TOML key deserializes as `Custom` for legacy files (see
    /// [`default_quality_preset`]).
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

/// Whether `path` names `makemkvcon` or `makemkvcon64`, with or without `.exe`,
/// in any case.
fn is_makemkvcon(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    matches!(stem, "makemkvcon" | "makemkvcon64")
}

/// Serde default for [`AppConfig::quality_preset`] on legacy config files.
fn default_quality_preset() -> QualityPreset {
    QualityPreset::Custom
}

/// The directory named by environment variable `name`. An empty or relative
/// value counts as unset, as the XDG base directory specification requires.
pub fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
}

/// A config home unique to the calling test thread, removed when the thread
/// exits.
#[cfg(test)]
fn test_config_home() -> PathBuf {
    struct TestDir(PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    thread_local! {
        static DIR: TestDir = TestDir(std::env::temp_dir().join(format!(
            "av1c_test_config_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        )));
    }
    DIR.with(|dir| dir.0.clone())
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
    /// Reports a parse failure on stderr, for the command-line entry points.
    pub fn load() -> Self {
        let (config, error) = Self::load_reporting();
        if let Some(error) = error {
            eprintln!(
                "{} ({})",
                crate::i18n::t(config.language, crate::i18n::Msg::ConfigLoadFailed),
                error.lines().next().unwrap_or_default()
            );
        }
        config
    }

    /// [`AppConfig::load`], plus the parse error when the file exists but
    /// could not be read.
    pub fn load_reporting() -> (Self, Option<String>) {
        if let Some(loaded) = Self::read_existing() {
            return loaded;
        }

        let config = Self {
            encoder: encoder_detect::detect_encoder(),
            ..Self::default()
        };
        if let Err(e) = config.save() {
            warn!("Failed to save default config: {e:?}");
        }
        (config, None)
    }

    /// Load the configuration file without creating it; defaults when there is
    /// none.
    pub fn load_existing() -> Self {
        Self::read_existing()
            .map(|(config, _)| config)
            .unwrap_or_default()
    }

    /// The configuration file as loaded, with the parse error when it could
    /// not be read; `None` when there is no file.
    fn read_existing() -> Option<(Self, Option<String>)> {
        let config_path = Self::config_path();
        if !config_path.exists() {
            return None;
        }
        Some(match Self::load_from_file(&config_path) {
            Ok(mut config) => {
                config.sanitize();
                info!("Loaded config from {}", config_path.display());
                (config, None)
            }
            Err(e) => {
                // A file that failed to parse is left untouched, not
                // overwritten with defaults.
                warn!("Failed to load config: {e:?}. Using defaults.");
                (Self::default(), Some(e.to_string()))
            }
        })
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
        result.map_err(|e| AppError::Config(format!("Failed to write config file: {e}")))?;
        Self::settle_directory(config_path);
        Ok(())
    }

    /// Flush the renamed entry to disk and remove the temporary files left by
    /// interrupted saves.
    fn settle_directory(config_path: &std::path::Path) {
        let parent = match config_path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            Some(_) => std::path::Path::new("."),
            None => return,
        };
        #[cfg(unix)]
        if let Err(e) = std::fs::File::open(parent).and_then(|dir| dir.sync_all()) {
            warn!(
                "Could not flush the config directory {}: {e}",
                parent.display()
            );
        }
        let prefix = format!(
            "{}.",
            config_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        let Ok(entries) = std::fs::read_dir(parent) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with(&prefix) && path.extension().is_some_and(|ext| ext == "tmp") {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Copy `path` aside as `<name>.unreadable-<secs>[-<n>]` if it exists but
    /// cannot be parsed.
    fn preserve_unreadable(path: &std::path::Path) -> Result<(), AppError> {
        if !path.exists() || Self::load_from_file(path).is_ok() {
            return Ok(());
        }
        let contents = std::fs::read(path).map_err(|e| {
            AppError::Config(format!("Could not read {} for backup: {e}", path.display()))
        })?;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        for n in 0u32.. {
            let suffix = if n == 0 {
                format!("{name}.unreadable-{secs}")
            } else {
                format!("{name}.unreadable-{secs}-{n}")
            };
            let backup = path.with_file_name(suffix);
            let mut file = match options.open(&backup) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(AppError::Config(format!(
                        "Refusing to overwrite the unreadable config at {}: could not create backup {} ({e})",
                        path.display(),
                        backup.display()
                    )));
                }
            };
            if let Err(e) = file.write_all(&contents) {
                let _ = std::fs::remove_file(&backup);
                return Err(AppError::Config(format!(
                    "Could not write backup {}: {e}",
                    backup.display()
                )));
            }
            warn!("Kept the unreadable config as {}", backup.display());
            return Ok(());
        }
        Err(AppError::Config(format!(
            "Refusing to overwrite the unreadable config at {}: no free backup name",
            path.display()
        )))
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
        #[cfg(test)]
        let base = test_config_home();
        #[cfg(not(test))]
        let base = env_dir("XDG_CONFIG_HOME")
            .or_else(|| env_dir("HOME").map(|home| home.join(".config")))
            .or_else(|| env_dir("APPDATA"))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("av1converter").join("config.toml")
    }

    /// Apply the quality preset, clamp all numeric fields to their valid ranges
    /// and repair output naming.
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
        if let Some(presets) = self.quality_preset.presets() {
            self.presets = presets;
        }
        for preset in self.presets.all_mut() {
            preset.crf = preset.crf.min(Encoder::SvtAv1.max_quality());
            let hw_max = Encoder::Nvenc.max_quality();
            preset.nvenc_cq = preset.nvenc_cq.min(hw_max);
            preset.qsv_quality = preset.qsv_quality.min(hw_max);
            preset.amf_quality = preset.amf_quality.min(hw_max);
            preset.film_grain = preset.film_grain.min(50);
        }
        // Path separators are stripped from the suffix, which becomes part of
        // the output filename. An unsupported container falls back to Matroska.
        strip_path_separators(&mut self.output.suffix);
        self.output.container = OutputConfig::supported_container(&self.output.container)
            .map_or_else(|| OutputConfig::default().container, str::to_string);
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
        // Multiplied by the channel count; clamped to a rate libopus
        // accepts.
        self.audio.opus_bitrate_per_channel = self
            .audio
            .opus_bitrate_per_channel
            .clamp(AudioConfig::MIN_PER_CHANNEL, AudioConfig::MAX_PER_CHANNEL);
        self.daemon.browse_root = self.daemon.browse_root.trim().to_string();
        self.daemon.auth_token = self.daemon.auth_token.trim().to_string();
        self.output.output_directory = self
            .output
            .output_directory
            .take()
            .map(|directory| directory.trim().to_string())
            .filter(|directory| !directory.is_empty());
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
        if self.quality.delete_source_on_success
            && self.quality.vmaf_enabled
            && self.quality.vmaf_threshold <= 0.0
        {
            return Err(t(self.language, Msg::ThresholdTooLowToDelete).to_string());
        }
        if self.performance.svt_preset > 13 {
            return Err("SVT preset must be between 0 and 13".to_string());
        }
        if !PerformanceConfig::valid_nvenc_preset(&self.performance.nvenc_preset) {
            return Err("NVENC preset must be between p1 and p7".to_string());
        }
        if OutputConfig::supported_container(&self.output.container).is_none() {
            return Err(t(self.language, Msg::InvalidContainer).to_string());
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
            return Err(t(self.language, Msg::InvalidPort).to_string());
        }
        if !self.daemon.auth_token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(t(self.language, Msg::InvalidAuthToken).to_string());
        }
        if !self.daemon.auth_token.is_empty() && self.daemon.auth_token.len() < 32 {
            return Err(t(self.language, Msg::TokenTooShort).to_string());
        }
        if self
            .daemon
            .bind_address
            .parse::<std::net::IpAddr>()
            .is_err()
        {
            return Err(t(self.language, Msg::InvalidAddress).to_string());
        }
        if self.daemon.binds_publicly() && self.daemon.browse_root.trim().is_empty() {
            return Err(t(self.language, Msg::BrowseRootRequired).to_string());
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
                .map_err(|_| t(self.language, Msg::BrowseRootInvalid).to_string())?;
            if !root.is_dir() {
                return Err(t(self.language, Msg::BrowseRootInvalid).to_string());
            }
            self.daemon.browse_root = root.to_string_lossy().into_owned();
        }
        if self.disc.staging_directory != previous.disc.staging_directory
            && let Some(directory) = &self.disc.staging_directory
        {
            let directory = std::path::Path::new(directory)
                .canonicalize()
                .map_err(|_| t(self.language, Msg::StagingDirectoryInvalid).to_string())?;
            if !directory.is_dir() {
                return Err(t(self.language, Msg::StagingDirectoryInvalid).to_string());
            }
            self.disc.staging_directory = Some(directory.to_string_lossy().into_owned());
        }
        if self.disc.makemkvcon_path != previous.disc.makemkvcon_path
            && let Some(executable) = &self.disc.makemkvcon_path
        {
            let executable = std::path::Path::new(executable)
                .canonicalize()
                .map_err(|_| t(self.language, Msg::MakemkvconInvalid).to_string())?;
            if !executable.is_file() || !is_makemkvcon(&executable) {
                return Err(t(self.language, Msg::MakemkvconInvalid).to_string());
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

    /// Each test thread gets its own config file under the temp directory, and
    /// it is gone once the thread exits.
    #[test]
    fn tests_use_a_private_config_path_per_thread() {
        let here = AppConfig::config_path();
        assert!(here.starts_with(std::env::temp_dir()));

        let there = std::thread::spawn(|| {
            AppConfig::default().save().unwrap();
            let path = AppConfig::config_path();
            assert!(path.exists());
            path
        })
        .join()
        .unwrap();

        assert_ne!(here, there);
        assert!(!there.exists());
    }

    /// A `[presets.<tier>]` table with only some values keeps that tier's
    /// defaults for the others and leaves the rest of the file intact.
    #[test]
    fn a_partial_preset_table_keeps_the_tier_defaults() {
        std::fs::create_dir_all(AppConfig::config_path().parent().unwrap()).unwrap();
        std::fs::write(
            AppConfig::config_path(),
            "[quality]\nvmaf_threshold = 91.0\n\n[presets.sd]\ncrf = 20\n\n[presets.uhd_dv]\nfilm_grain = 9\n",
        )
        .unwrap();
        let (config, error) = AppConfig::read_existing().unwrap();
        assert_eq!(error, None);
        assert_eq!(config.quality_preset, QualityPreset::Custom);
        assert!((config.quality.vmaf_threshold - 91.0).abs() < f64::EPSILON);

        let defaults = EncodingPresetsConfig::default();
        assert_eq!(
            config.presets.sd,
            EncodingPreset {
                crf: 20,
                ..defaults.sd.clone()
            }
        );
        assert_eq!(
            config.presets.uhd_dv,
            EncodingPreset {
                film_grain: 9,
                ..defaults.uhd_dv.clone()
            }
        );
        assert_eq!(config.presets.hd, defaults.hd);

        std::fs::write(AppConfig::config_path(), "[presets.sd]\ncrf = \"x\"\n").unwrap();
        assert!(AppConfig::read_existing().unwrap().1.is_some());
        let _ = std::fs::remove_file(AppConfig::config_path());
    }

    /// A blank output directory counts as unset; surrounding spaces are trimmed.
    #[test]
    fn a_blank_output_directory_counts_as_unset() {
        let mut cfg = AppConfig::default();
        cfg.output.output_directory = Some("  ".to_string());
        cfg.sanitize();
        assert_eq!(cfg.output.output_directory, None);

        cfg.output.output_directory = Some(" /data/out ".to_string());
        cfg.sanitize();
        assert_eq!(cfg.output.output_directory.as_deref(), Some("/data/out"));
    }

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

    /// Reading without creating leaves no file behind.
    #[test]
    fn load_existing_does_not_create_a_config() {
        let path = AppConfig::config_path();
        let _ = std::fs::remove_file(&path);
        assert_eq!(AppConfig::load_existing(), AppConfig::default());
        assert!(!path.exists());
    }

    /// A section with only some keys keeps the rest at their defaults.
    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let loaded: AppConfig = toml::from_str("[daemon]\nenabled = true\n").unwrap();
        assert!(loaded.daemon.enabled);
        assert_eq!(loaded.daemon.port, DaemonConfig::default().port);
        assert_eq!(loaded.quality, QualityConfig::default());
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

    /// A save removes the temporary files an interrupted save left behind,
    /// and keeps the ones it is not responsible for.
    #[test]
    fn a_save_sweeps_leftover_temporary_files() {
        let dir = std::env::temp_dir().join(format!("av1c_cfg_tmp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let stale = dir.join("config.toml.0123456789abcdef.tmp");
        let foreign = dir.join("queue.json.tmp");
        std::fs::write(&stale, b"half a config").unwrap();
        std::fs::write(&foreign, b"not ours").unwrap();

        AppConfig::default().save_to_path(&path).unwrap();

        assert!(
            !stale.exists(),
            "the leftover temporary file is still there"
        );
        assert_eq!(std::fs::read(&foreign).unwrap(), b"not ours");
        assert!(AppConfig::load_from_file(&path).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bytes of every config a user broke by hand survive the save that
    /// overwrites it, including a second and a third one.
    #[test]
    fn every_unreadable_config_is_preserved_before_it_is_overwritten() {
        let dir = std::env::temp_dir().join(format!("av1c_cfg_backup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let copies = || {
            let mut kept: Vec<Vec<u8>> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|entry| {
                    entry
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .starts_with("config.toml.unreadable-")
                })
                .map(|entry| std::fs::read(entry).unwrap())
                .collect();
            kept.sort();
            kept
        };

        // A file that parses needs no copy.
        AppConfig::default().save_to_path(&path).unwrap();
        assert!(copies().is_empty());

        // Each one that does not is kept verbatim, under a name of its own.
        let broken: [&[u8]; 3] = [
            b"this is not = valid toml [[[",
            b"another invalid config [[[",
            b"a third invalid config [[[",
        ];
        for (n, contents) in broken.iter().enumerate() {
            std::fs::write(&path, contents).unwrap();
            AppConfig::default().save_to_path(&path).unwrap();
            assert!(AppConfig::load_from_file(&path).is_ok());
            let mut expected: Vec<Vec<u8>> =
                broken[..=n].iter().map(|bytes| bytes.to_vec()).collect();
            expected.sort();
            assert_eq!(copies(), expected);
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap().path();
                assert_eq!(
                    std::fs::metadata(&entry).unwrap().permissions().mode() & 0o077,
                    0,
                    "{} is readable by others",
                    entry.display()
                );
            }
        }

        // A missing file is simply nothing to protect.
        std::fs::remove_file(&path).unwrap();
        AppConfig::preserve_unreadable(&path).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A per-channel bitrate outside libopus' useful range is clamped.
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

    /// Only containers the encoder supports are accepted; `sanitize` spells a
    /// supported one canonically and replaces anything else with Matroska.
    #[test]
    fn output_container_is_limited_to_supported_formats() {
        let mut cfg = AppConfig::default();
        cfg.output.container = " .WebM".to_string();
        assert!(cfg.validate_settings().is_ok());
        cfg.sanitize();
        assert_eq!(cfg.output.container, "webm");

        cfg.output.container = "avi".to_string();
        assert!(cfg.validate_settings().is_err());
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

    /// A named quality preset rewrites every tier; `custom` keeps them.
    #[test]
    fn a_named_quality_preset_overrides_the_per_tier_values() {
        let mut cfg = AppConfig {
            quality_preset: QualityPreset::High,
            ..AppConfig::default()
        };
        cfg.presets.sd.crf = 1;
        cfg.sanitize();
        assert_eq!(cfg.presets, EncodingPresetsConfig::high());

        cfg.quality_preset = QualityPreset::Custom;
        cfg.presets.sd.crf = 1;
        cfg.sanitize();
        assert_eq!(cfg.presets.sd.crf, 1);
    }

    #[test]
    fn invalid_nvenc_preset_falls_back_to_the_supported_default() {
        let mut cfg = AppConfig::default();
        cfg.performance.nvenc_preset = "slowest".to_string();
        cfg.sanitize();
        assert_eq!(cfg.performance.nvenc_preset, "p4");
    }

    /// The printed URL is a link the user can open; a wildcard bind address
    /// is shown as loopback.
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
        assert!(!cfg.daemon.refuses_public_bind());
        cfg.daemon.bind_address = "0.0.0.0".to_string();
        assert!(cfg.daemon.binds_publicly());
        assert!(cfg.daemon.refuses_public_bind());
        cfg.daemon.allow_insecure_lan = true;
        assert!(!cfg.daemon.refuses_public_bind());
        cfg.daemon.allow_insecure_lan = false;
        cfg.daemon.bind_address = "192.168.1.10".to_string();
        assert!(cfg.daemon.binds_publicly());
        cfg.daemon.bind_address = "::1".to_string();
        assert!(!cfg.daemon.binds_publicly());
        assert_eq!(cfg.daemon.listen_address(), "[::1]:8399");
    }

    #[test]
    fn public_bind_requires_a_browse_root() {
        let mut cfg = AppConfig::default();
        cfg.daemon.bind_address = "0.0.0.0".to_string();
        cfg.daemon.browse_root.clear();
        assert!(cfg.validate_settings().is_err());
        cfg.daemon.browse_root = "/tmp".to_string();
        assert!(cfg.validate_settings().is_ok());
    }

    #[test]
    fn an_access_token_must_be_printable_ascii() {
        let mut cfg = AppConfig::default();
        cfg.daemon.auth_token = "é".repeat(32);
        assert!(cfg.validate_settings().is_err());
        cfg.daemon.auth_token = "a b".repeat(11);
        assert!(cfg.validate_settings().is_err());
        cfg.daemon.auth_token = "a".repeat(32);
        assert!(cfg.validate_settings().is_ok());
        cfg.daemon.auth_token.clear();
        assert!(cfg.validate_settings().is_ok());
    }

    /// Deleting the source needs a threshold that can actually fail.
    #[test]
    fn deleting_the_source_needs_a_threshold_above_zero() {
        let mut config = AppConfig::default();
        config.quality.vmaf_enabled = true;
        config.quality.delete_source_on_success = true;
        config.quality.vmaf_threshold = 0.0;
        assert_eq!(
            config.validate_settings(),
            Err(t(config.language, Msg::ThresholdTooLowToDelete).to_string())
        );

        config.quality.vmaf_threshold = 0.5;
        assert_eq!(config.validate_settings(), Ok(()));

        config.quality.vmaf_threshold = 0.0;
        config.quality.delete_source_on_success = false;
        assert_eq!(config.validate_settings(), Ok(()));
    }

    /// A non-empty token needs at least 32 characters; an empty one is
    /// regenerated when the daemon starts.
    #[test]
    fn an_access_token_is_empty_or_at_least_32_characters() {
        let mut cfg = AppConfig::default();
        cfg.daemon.auth_token = "a".repeat(31);
        assert_eq!(
            cfg.validate_settings(),
            Err(t(cfg.language, Msg::TokenTooShort).to_string())
        );
        cfg.daemon.auth_token = "a".repeat(32);
        assert_eq!(cfg.validate_settings(), Ok(()));
        cfg.daemon.auth_token.clear();
        assert_eq!(cfg.validate_settings(), Ok(()));
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
    fn the_makemkv_executable_must_be_makemkvcon() {
        let dir = std::env::temp_dir().join(format!("av1c_makemkv_name_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let previous = AppConfig::default();
        for (name, accepted) in [
            ("makemkvcon", true),
            ("makemkvcon64.exe", true),
            ("MakeMKVcon.EXE", true),
            ("evil.sh", false),
            ("makemkvcon-evil", false),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, b"").unwrap();
            let mut cfg = previous.clone();
            cfg.disc.makemkvcon_path = Some(path.to_string_lossy().into_owned());
            assert_eq!(
                cfg.normalize_changed_host_paths(&previous).is_ok(),
                accepted,
                "{name}"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
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
