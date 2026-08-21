use super::common::message_color;
use crate::app::App;
use crate::config::{AppConfig, AudioMode, EncodingPreset, QualityPreset};
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

/// How a config item's value is changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigItemKind {
    /// ← → cycles through a fixed set of named options
    Cycle,
    /// ← → increments / decrements a number
    Numeric,
    /// ← → flips a boolean (Yes / No)
    Toggle,
    /// Enter opens an inline text editor
    Text,
}

/// Which `AppConfig` field a row maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Language,
    Encoder,
    VmafThreshold,
    VmafEnabled,
    DeleteSource,
    SvtPreset,
    NvencPreset,
    QualityPreset,
    Preset(PresetTier, PresetMetric),
    OutputSuffix,
    OutputContainer,
    SameDirectory,
    OutputDirectory,
    AudioLanguages,
    SubtitleLanguages,
    SelectAllFallback,
    AudioDefaultMode,
    OpusBitratePerChannel,
    SkipAlreadyOpus,
    DaemonEnabled,
    DaemonAutostart,
    DaemonBindAddress,
    DaemonPort,
    DaemonBrowseRoot,
    DaemonAuthToken,
    DiscMakemkvconPath,
    DiscStagingDirectory,
}

/// Resolution and dynamic-range tier used by an encoding preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetTier {
    Sd,
    Hd,
    FullHd,
    FullHdHdr,
    FullHdDv,
    Uhd,
    UhdHdr,
    UhdDv,
}

/// Encoder property stored for each resolution tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetMetric {
    Crf,
    FilmGrain,
    NvencCq,
    QsvQuality,
    AmfQuality,
}

/// Descriptor for a single row in the config screen.
pub struct ConfigItem {
    pub label: Msg,
    pub kind: ConfigItemKind,
    pub field: ConfigField,
}

/// Master list of every config item, in display order.
pub const CONFIG_ITEMS: &[ConfigItem] = &[
    ConfigItem {
        label: Msg::CfgLanguage,
        kind: ConfigItemKind::Cycle,
        field: ConfigField::Language,
    },
    ConfigItem {
        label: Msg::EncoderLabel,
        kind: ConfigItemKind::Cycle,
        field: ConfigField::Encoder,
    },
    ConfigItem {
        label: Msg::CfgSvtPreset,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::SvtPreset,
    },
    ConfigItem {
        label: Msg::CfgNvencPreset,
        kind: ConfigItemKind::Cycle,
        field: ConfigField::NvencPreset,
    },
    ConfigItem {
        label: Msg::CfgQualityPreset,
        kind: ConfigItemKind::Cycle,
        field: ConfigField::QualityPreset,
    },
    ConfigItem {
        label: Msg::CfgVmafEnabled,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::VmafEnabled,
    },
    ConfigItem {
        label: Msg::CfgVmafThreshold,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::VmafThreshold,
    },
    ConfigItem {
        label: Msg::CfgDeleteSource,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::DeleteSource,
    },
    ConfigItem {
        label: Msg::CfgRfSd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Sd, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfSd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Sd, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfSd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Sd, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfSd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Sd, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfSd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Sd, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Hd, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Hd, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Hd, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Hd, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Hd, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfFullHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHd, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfFullHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHd, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfFullHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHd, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfFullHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHd, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfFullHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHd, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdHdr, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdHdr, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdHdr, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdHdr, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdHdr, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdDv, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdDv, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdDv, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdDv, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfFullHdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::FullHdDv, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfUhd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Uhd, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfUhd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Uhd, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfUhd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Uhd, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfUhd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Uhd, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfUhd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::Uhd, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfUhdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdHdr, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfUhdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdHdr, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfUhdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdHdr, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfUhdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdHdr, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfUhdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdHdr, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgRfUhdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdDv, PresetMetric::Crf),
    },
    ConfigItem {
        label: Msg::CfgRfUhdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdDv, PresetMetric::FilmGrain),
    },
    ConfigItem {
        label: Msg::CfgRfUhdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdDv, PresetMetric::NvencCq),
    },
    ConfigItem {
        label: Msg::CfgRfUhdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdDv, PresetMetric::QsvQuality),
    },
    ConfigItem {
        label: Msg::CfgRfUhdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::Preset(PresetTier::UhdDv, PresetMetric::AmfQuality),
    },
    ConfigItem {
        label: Msg::CfgOutputSuffix,
        kind: ConfigItemKind::Text,
        field: ConfigField::OutputSuffix,
    },
    ConfigItem {
        label: Msg::CfgOutputContainer,
        kind: ConfigItemKind::Text,
        field: ConfigField::OutputContainer,
    },
    ConfigItem {
        label: Msg::CfgSameDirectory,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::SameDirectory,
    },
    ConfigItem {
        label: Msg::WebCfgOutputDirectory,
        kind: ConfigItemKind::Text,
        field: ConfigField::OutputDirectory,
    },
    ConfigItem {
        label: Msg::CfgAudioLanguages,
        kind: ConfigItemKind::Text,
        field: ConfigField::AudioLanguages,
    },
    ConfigItem {
        label: Msg::CfgSubtitleLanguages,
        kind: ConfigItemKind::Text,
        field: ConfigField::SubtitleLanguages,
    },
    ConfigItem {
        label: Msg::WebCfgSelectAllFallback,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::SelectAllFallback,
    },
    ConfigItem {
        label: Msg::AudioMode,
        kind: ConfigItemKind::Cycle,
        field: ConfigField::AudioDefaultMode,
    },
    ConfigItem {
        label: Msg::OpusBitratePerChannel,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::OpusBitratePerChannel,
    },
    ConfigItem {
        label: Msg::SkipAlreadyOpus,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::SkipAlreadyOpus,
    },
    ConfigItem {
        label: Msg::CfgDaemonEnabled,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::DaemonEnabled,
    },
    ConfigItem {
        label: Msg::CfgDaemonAutostart,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::DaemonAutostart,
    },
    ConfigItem {
        label: Msg::CfgDaemonBindAddress,
        kind: ConfigItemKind::Text,
        field: ConfigField::DaemonBindAddress,
    },
    ConfigItem {
        label: Msg::CfgDaemonPort,
        kind: ConfigItemKind::Text,
        field: ConfigField::DaemonPort,
    },
    ConfigItem {
        label: Msg::CfgDaemonBrowseRoot,
        kind: ConfigItemKind::Text,
        field: ConfigField::DaemonBrowseRoot,
    },
    ConfigItem {
        label: Msg::CfgDaemonAuthToken,
        kind: ConfigItemKind::Text,
        field: ConfigField::DaemonAuthToken,
    },
    ConfigItem {
        label: Msg::CfgMakemkvconPath,
        kind: ConfigItemKind::Text,
        field: ConfigField::DiscMakemkvconPath,
    },
    ConfigItem {
        label: Msg::CfgStagingDirectory,
        kind: ConfigItemKind::Text,
        field: ConfigField::DiscStagingDirectory,
    },
];

/// Whether a field is one of the per-resolution rate-factor rows.
fn is_rf_field(field: ConfigField) -> bool {
    matches!(field, ConfigField::Preset(_, _))
}

#[cfg(test)]
fn config_field_path(field: ConfigField) -> Option<String> {
    let path = match field {
        ConfigField::Language => "language",
        ConfigField::Encoder => "encoder",
        ConfigField::VmafThreshold => "quality.vmaf_threshold",
        ConfigField::VmafEnabled => "quality.vmaf_enabled",
        ConfigField::DeleteSource => "quality.delete_source_on_success",
        ConfigField::SvtPreset => "performance.svt_preset",
        ConfigField::NvencPreset => "performance.nvenc_preset",
        ConfigField::QualityPreset => "quality_preset",
        ConfigField::OutputSuffix => "output.suffix",
        ConfigField::OutputContainer => "output.container",
        ConfigField::SameDirectory => "output.same_directory",
        ConfigField::OutputDirectory => "output.output_directory",
        ConfigField::AudioLanguages => "tracks.preferred_audio_languages",
        ConfigField::SubtitleLanguages => "tracks.preferred_subtitle_languages",
        ConfigField::SelectAllFallback => "tracks.select_all_fallback",
        ConfigField::AudioDefaultMode => "audio.default_mode",
        ConfigField::OpusBitratePerChannel => "audio.opus_bitrate_per_channel",
        ConfigField::SkipAlreadyOpus => "audio.skip_already_opus",
        ConfigField::DaemonEnabled => "daemon.enabled",
        ConfigField::DaemonBindAddress => "daemon.bind_address",
        ConfigField::DaemonPort => "daemon.port",
        ConfigField::DaemonBrowseRoot => "daemon.browse_root",
        ConfigField::DaemonAuthToken => "daemon.auth_token",
        ConfigField::DiscMakemkvconPath => "disc.makemkvcon_path",
        ConfigField::DiscStagingDirectory => "disc.staging_directory",
        ConfigField::DaemonAutostart => return None,
        ConfigField::Preset(tier, metric) => {
            let tier = match tier {
                PresetTier::Sd => "sd",
                PresetTier::Hd => "hd",
                PresetTier::FullHd => "full_hd",
                PresetTier::FullHdHdr => "full_hd_hdr",
                PresetTier::FullHdDv => "full_hd_dv",
                PresetTier::Uhd => "uhd",
                PresetTier::UhdHdr => "uhd_hdr",
                PresetTier::UhdDv => "uhd_dv",
            };
            let metric = match metric {
                PresetMetric::Crf => "crf",
                PresetMetric::FilmGrain => "film_grain",
                PresetMetric::NvencCq => "nvenc_cq",
                PresetMetric::QsvQuality => "qsv_quality",
                PresetMetric::AmfQuality => "amf_quality",
            };
            return Some(format!("presets.{tier}.{metric}"));
        }
    };
    Some(path.to_string())
}

/// The config rows actually shown for the current config.
///
/// The per-tier rate-factor rows are only visible when the quality preset is
/// [`QualityPreset::Custom`]; otherwise their values are driven by the preset.
pub fn visible_config_items(config: &AppConfig) -> Vec<&'static ConfigItem> {
    let show_rf = config.quality_preset == QualityPreset::Custom;
    CONFIG_ITEMS
        .iter()
        .filter(|item| show_rf || !is_rf_field(item.field))
        .filter(|item| {
            config.quality.vmaf_enabled
                || !matches!(
                    item.field,
                    ConfigField::VmafThreshold | ConfigField::DeleteSource
                )
        })
        .filter(|item| !config.output.same_directory || item.field != ConfigField::OutputDirectory)
        .filter(|item| {
            item.field != ConfigField::DaemonAutostart || crate::daemon::service::supported()
        })
        .collect()
}

/// Localized display name for a quality preset.
fn quality_preset_name(lang: crate::i18n::Language, preset: QualityPreset) -> &'static str {
    t(
        lang,
        match preset {
            QualityPreset::Low => Msg::QpLow,
            QualityPreset::Medium => Msg::QpMedium,
            QualityPreset::High => Msg::QpHigh,
            QualityPreset::Custom => Msg::QpCustom,
        },
    )
}

/// Localized display name for an audio mode.
fn audio_mode_name(lang: crate::i18n::Language, mode: AudioMode) -> String {
    match mode {
        AudioMode::Copy => t(lang, Msg::CopyTracks).to_string(),
        AudioMode::Opus => "Opus".to_string(),
    }
}

/// Read the current display value for visible config item `index` from `config`.
pub fn get_config_value(config: &AppConfig, index: usize) -> String {
    let items = visible_config_items(config);
    let Some(item) = items.get(index) else {
        return String::new();
    };
    match item.field {
        ConfigField::Language => config.language.display_name().to_string(),
        ConfigField::Encoder => config.encoder.display_name().to_string(),
        ConfigField::QualityPreset => {
            quality_preset_name(config.language, config.quality_preset).to_string()
        }
        ConfigField::VmafThreshold => format!("{:.0}", config.quality.vmaf_threshold),
        ConfigField::VmafEnabled => bool_display(config.language, config.quality.vmaf_enabled),
        ConfigField::DeleteSource => {
            bool_display(config.language, config.quality.delete_source_on_success)
        }
        ConfigField::SvtPreset => config.performance.svt_preset.to_string(),
        ConfigField::NvencPreset => config.performance.nvenc_preset.clone(),
        ConfigField::Preset(tier, metric) => {
            preset_value(preset_for_tier(&config.presets, tier), metric).to_string()
        }
        ConfigField::OutputSuffix => config.output.suffix.clone(),
        ConfigField::OutputContainer => config.output.container.clone(),
        ConfigField::SameDirectory => bool_display(config.language, config.output.same_directory),
        ConfigField::OutputDirectory => config
            .output
            .output_directory
            .as_deref()
            .map_or_else(|| "—".to_string(), empty_as_dash),
        ConfigField::AudioLanguages => config.tracks.preferred_audio_languages.join(", "),
        ConfigField::SubtitleLanguages => config.tracks.preferred_subtitle_languages.join(", "),
        ConfigField::SelectAllFallback => {
            bool_display(config.language, config.tracks.select_all_fallback)
        }
        ConfigField::AudioDefaultMode => {
            audio_mode_name(config.language, config.audio.default_mode)
        }
        ConfigField::OpusBitratePerChannel => config.audio.opus_bitrate_per_channel.to_string(),
        ConfigField::SkipAlreadyOpus => {
            bool_display(config.language, config.audio.skip_already_opus)
        }
        ConfigField::DaemonEnabled => bool_display(config.language, config.daemon.enabled),
        ConfigField::DaemonAutostart => {
            bool_display(config.language, crate::daemon::service::installed())
        }
        ConfigField::DaemonBindAddress => config.daemon.bind_address.clone(),
        ConfigField::DaemonPort => config.daemon.port.to_string(),
        ConfigField::DaemonBrowseRoot => empty_as_dash(&config.daemon.browse_root),
        ConfigField::DiscMakemkvconPath => config
            .disc
            .makemkvcon_path
            .as_deref()
            .map_or_else(|| "—".to_string(), empty_as_dash),
        ConfigField::DiscStagingDirectory => config
            .disc
            .staging_directory
            .as_deref()
            .map_or_else(|| "—".to_string(), empty_as_dash),
        // Shown as a placeholder rather than the secret itself.
        ConfigField::DaemonAuthToken => {
            if config.daemon.auth_token.is_empty() {
                "—".to_string()
            } else {
                "••••••••".to_string()
            }
        }
    }
}

/// Render an unset optional text field as a dash rather than as blank.
fn empty_as_dash(value: &str) -> String {
    if value.is_empty() {
        "—".to_string()
    } else {
        value.to_string()
    }
}

fn preset_for_tier(
    presets: &crate::config::EncodingPresetsConfig,
    tier: PresetTier,
) -> &EncodingPreset {
    match tier {
        PresetTier::Sd => &presets.sd,
        PresetTier::Hd => &presets.hd,
        PresetTier::FullHd => &presets.full_hd,
        PresetTier::FullHdHdr => &presets.full_hd_hdr,
        PresetTier::FullHdDv => &presets.full_hd_dv,
        PresetTier::Uhd => &presets.uhd,
        PresetTier::UhdHdr => &presets.uhd_hdr,
        PresetTier::UhdDv => &presets.uhd_dv,
    }
}

fn preset_value(preset: &EncodingPreset, metric: PresetMetric) -> u8 {
    match metric {
        PresetMetric::Crf => preset.crf,
        PresetMetric::FilmGrain => preset.film_grain,
        PresetMetric::NvencCq => preset.nvenc_cq,
        PresetMetric::QsvQuality => preset.qsv_quality,
        PresetMetric::AmfQuality => preset.amf_quality,
    }
}

fn bool_display(lang: crate::i18n::Language, v: bool) -> String {
    t(lang, if v { Msg::Yes } else { Msg::No }).to_string()
}

// ── Rendering ────────────────────────────────────────────────────────────────

pub fn render_config_screen(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    let lang = app.config.language;

    // Title
    let title = Paragraph::new(t(lang, Msg::Configuration))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(title, chunks[0]);

    // Config items
    let items = build_config_items(
        &app.config,
        app.config_selected,
        app.config_edit_buffer.is_some(),
        app.config_edit_buffer.as_deref().unwrap_or(""),
        app.config_edit_cursor,
        chunks[1].width.saturating_sub(2) as usize,
    );

    // Use a ListState to handle scrolling automatically
    let mut list_state = ListState::default();
    list_state.select(Some(app.config_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(if app.config_is_dirty() {
                    format!(
                        " {} · {} ",
                        t(lang, Msg::Settings),
                        t(lang, Msg::UnsavedChanges)
                    )
                } else {
                    format!(" {} ", t(lang, Msg::Settings))
                }),
        )
        .highlight_style(Style::default());

    f.render_stateful_widget(list, chunks[1], &mut list_state);

    let help_text = if app.config_edit_buffer.is_some() {
        Line::from(vec![
            Span::styled("←→", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Confirm))),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}", t(lang, Msg::Cancel))),
        ])
    } else {
        Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
            Span::styled("←→", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Adjust))),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::EditText))),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Save))),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Back))),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
        ])
    };

    let mut footer = Vec::new();
    if let Some(ref msg) = app.message {
        footer.push(Line::from(Span::styled(
            msg.clone(),
            Style::default()
                .fg(message_color(app.message_kind))
                .add_modifier(Modifier::BOLD),
        )));
    } else if let Some(hint) = selected_config_hint(app) {
        footer.push(Line::from(Span::styled(
            t(lang, hint),
            Style::default().fg(Color::Yellow),
        )));
    }
    footer.push(help_text);
    f.render_widget(
        Paragraph::new(footer)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[2],
    );
}

/// Yellow footer for rows that need a warning rather than the key help.
fn selected_config_hint(app: &App) -> Option<Msg> {
    match visible_config_items(&app.config)
        .get(app.config_selected)
        .map(|item| item.field)
    {
        Some(ConfigField::DeleteSource) => Some(Msg::WebDeleteSourceWarning),
        Some(ConfigField::DaemonAutostart) => Some(Msg::CfgDaemonAutostartHint),
        _ => None,
    }
}

fn build_config_items(
    config: &AppConfig,
    selected: usize,
    editing: bool,
    input_buffer: &str,
    input_cursor: usize,
    row_width: usize,
) -> Vec<ListItem<'static>> {
    visible_config_items(config)
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == selected;
            let is_text = item.kind == ConfigItemKind::Text;

            let prefix = if is_selected { "> " } else { "  " };
            let label = format!("{}{}: ", prefix, config_item_label(config.language, item));
            let hint = if is_text && is_selected && !editing {
                t(config.language, Msg::EnterToEdit)
            } else {
                ""
            };

            let display_value = if is_selected && editing && is_text {
                let value_width = row_width
                    .saturating_sub(Line::raw(&label).width())
                    .saturating_sub(Line::raw(hint).width());
                edit_display(item.field, input_buffer, input_cursor, value_width)
            } else {
                get_config_value(config, i)
            };

            let label_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let value_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let row = Line::from(vec![
                Span::styled(label, label_style),
                Span::styled(display_value, value_style),
                Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray)),
            ]);
            let mut lines = Vec::new();
            if let Some(group) = config_group(item.field) {
                lines.push(Line::from(Span::styled(
                    t(config.language, group),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(row);
            ListItem::new(lines)
        })
        .collect()
}

fn config_item_label(lang: crate::i18n::Language, item: &ConfigItem) -> String {
    let tier = t(lang, item.label);
    match item.field {
        ConfigField::Preset(_, metric) => {
            let metric = match metric {
                PresetMetric::Crf => "CRF",
                PresetMetric::FilmGrain => t(lang, Msg::CfgFilmGrain),
                PresetMetric::NvencCq => "NVENC CQ",
                PresetMetric::QsvQuality => "QSV Quality",
                PresetMetric::AmfQuality => "AMF Quality",
            };
            format!("{tier} · {metric}")
        }
        _ => tier.to_string(),
    }
}

fn edit_display(field: ConfigField, value: &str, cursor: usize, max_width: usize) -> String {
    if max_width < 2 {
        return if max_width == 0 { "" } else { "|" }.to_string();
    }
    let mut characters: Vec<char> = if field == ConfigField::DaemonAuthToken {
        value.chars().map(|_| '•').collect()
    } else {
        value.chars().collect()
    };
    let cursor = cursor.min(characters.len());
    characters.insert(cursor, '|');
    let full: String = characters.iter().collect();
    if Line::raw(&full).width() <= max_width {
        return full;
    }

    let mut start = 0;
    while start < cursor {
        let prefix_width = usize::from(start > 0);
        let visible: String = characters[start..=cursor].iter().collect();
        if prefix_width + Line::raw(&visible).width() <= max_width {
            break;
        }
        start += 1;
    }

    let mut display = if start > 0 {
        "…".to_string()
    } else {
        String::new()
    };
    for (index, character) in characters.iter().enumerate().skip(start) {
        let more = index + 1 < characters.len();
        let candidate = format!("{display}{character}{}", if more { "…" } else { "" });
        if Line::raw(&candidate).width() > max_width {
            if more && Line::raw(format!("{display}…")).width() <= max_width {
                display.push('…');
            }
            break;
        }
        display.push(*character);
    }
    display
}

fn config_group(field: ConfigField) -> Option<Msg> {
    match field {
        ConfigField::Language => Some(Msg::WebGroupGeneral),
        ConfigField::SvtPreset | ConfigField::NvencPreset => Some(Msg::WebGroupPerformance),
        ConfigField::QualityPreset => Some(Msg::WebGroupQuality),
        ConfigField::Preset(PresetTier::Sd, PresetMetric::Crf) => Some(Msg::WebGroupRateFactors),
        ConfigField::OutputSuffix => Some(Msg::WebGroupOutput),
        ConfigField::AudioLanguages => Some(Msg::CfgGroupTracks),
        ConfigField::AudioDefaultMode => Some(Msg::WebGroupAudio),
        ConfigField::DaemonEnabled => Some(Msg::WebGroupDaemon),
        ConfigField::DiscMakemkvconPath => Some(Msg::CfgGroupDisc),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_tokens_are_masked_at_rest_and_while_editing() {
        let mut config = AppConfig::default();
        config.daemon.auth_token = "secret-value".to_string();
        let index = visible_config_items(&config)
            .iter()
            .position(|item| item.field == ConfigField::DaemonAuthToken)
            .unwrap();

        assert!(!get_config_value(&config, index).contains("secret-value"));
        assert!(
            !edit_display(ConfigField::DaemonAuthToken, "secret-value", 6, 40)
                .contains("secret-value")
        );
    }

    #[test]
    fn edited_values_keep_the_cursor_inside_the_visible_row() {
        let display = edit_display(ConfigField::OutputDirectory, "/a/very/long/path", 17, 8);
        assert!(display.contains('|'));
        assert!(Line::raw(display).width() <= 8);
    }

    #[test]
    fn configuration_rows_have_section_boundaries() {
        assert_eq!(
            config_group(ConfigField::Language),
            Some(Msg::WebGroupGeneral)
        );
        assert_eq!(
            config_group(ConfigField::DaemonEnabled),
            Some(Msg::WebGroupDaemon)
        );
        assert_eq!(
            config_group(ConfigField::AudioLanguages),
            Some(Msg::CfgGroupTracks)
        );
        assert_eq!(
            config_group(ConfigField::AudioDefaultMode),
            Some(Msg::WebGroupAudio)
        );
        assert_eq!(
            config_group(ConfigField::DiscMakemkvconPath),
            Some(Msg::CfgGroupDisc)
        );
    }

    #[test]
    fn configuration_rows_cover_every_serialized_setting() {
        let mut config = AppConfig {
            quality_preset: QualityPreset::Custom,
            ..AppConfig::default()
        };
        config.output.same_directory = false;

        let mut actual: Vec<String> = CONFIG_ITEMS
            .iter()
            .filter_map(|item| config_field_path(item.field))
            .collect();
        actual.sort();
        actual.dedup();

        let mut expected: Vec<String> = crate::config::settings::SERIALIZED_SETTING_PATHS
            .iter()
            .map(|path| (*path).to_string())
            .collect();
        expected.sort();

        assert_eq!(actual, expected);
    }
}
