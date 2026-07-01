use crate::app::App;
use crate::config::{AppConfig, Encoder, EncodingPreset, QualityPreset};
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
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
    RfSd,
    RfHd,
    RfFullHd,
    RfFullHdHdr,
    RfFullHdDv,
    RfUhd,
    RfUhdHdr,
    RfUhdDv,
    OutputSuffix,
    OutputContainer,
    SameDirectory,
    AudioLanguages,
    SubtitleLanguages,
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
        label: Msg::CfgVmafThreshold,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::VmafThreshold,
    },
    ConfigItem {
        label: Msg::CfgVmafEnabled,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::VmafEnabled,
    },
    ConfigItem {
        label: Msg::CfgDeleteSource,
        kind: ConfigItemKind::Toggle,
        field: ConfigField::DeleteSource,
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
        label: Msg::CfgRfSd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfSd,
    },
    ConfigItem {
        label: Msg::CfgRfHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfHd,
    },
    ConfigItem {
        label: Msg::CfgRfFullHd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfFullHd,
    },
    ConfigItem {
        label: Msg::CfgRfFullHdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfFullHdHdr,
    },
    ConfigItem {
        label: Msg::CfgRfFullHdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfFullHdDv,
    },
    ConfigItem {
        label: Msg::CfgRfUhd,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfUhd,
    },
    ConfigItem {
        label: Msg::CfgRfUhdHdr,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfUhdHdr,
    },
    ConfigItem {
        label: Msg::CfgRfUhdDv,
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfUhdDv,
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
        label: Msg::CfgAudioLanguages,
        kind: ConfigItemKind::Text,
        field: ConfigField::AudioLanguages,
    },
    ConfigItem {
        label: Msg::CfgSubtitleLanguages,
        kind: ConfigItemKind::Text,
        field: ConfigField::SubtitleLanguages,
    },
];

/// Whether a field is one of the per-resolution rate-factor rows.
fn is_rf_field(field: ConfigField) -> bool {
    matches!(
        field,
        ConfigField::RfSd
            | ConfigField::RfHd
            | ConfigField::RfFullHd
            | ConfigField::RfFullHdHdr
            | ConfigField::RfFullHdDv
            | ConfigField::RfUhd
            | ConfigField::RfUhdHdr
            | ConfigField::RfUhdDv
    )
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
        ConfigField::RfSd => preset_rf(config.encoder, &config.presets.sd),
        ConfigField::RfHd => preset_rf(config.encoder, &config.presets.hd),
        ConfigField::RfFullHd => preset_rf(config.encoder, &config.presets.full_hd),
        ConfigField::RfFullHdHdr => preset_rf(config.encoder, &config.presets.full_hd_hdr),
        ConfigField::RfFullHdDv => preset_rf(config.encoder, &config.presets.full_hd_dv),
        ConfigField::RfUhd => preset_rf(config.encoder, &config.presets.uhd),
        ConfigField::RfUhdHdr => preset_rf(config.encoder, &config.presets.uhd_hdr),
        ConfigField::RfUhdDv => preset_rf(config.encoder, &config.presets.uhd_dv),
        ConfigField::OutputSuffix => config.output.suffix.clone(),
        ConfigField::OutputContainer => config.output.container.clone(),
        ConfigField::SameDirectory => bool_display(config.language, config.output.same_directory),
        ConfigField::AudioLanguages => config.tracks.preferred_audio_languages.join(", "),
        ConfigField::SubtitleLanguages => config.tracks.preferred_subtitle_languages.join(", "),
    }
}

fn preset_rf(encoder: Encoder, preset: &EncodingPreset) -> String {
    match encoder {
        Encoder::SvtAv1 => preset.crf.to_string(),
        Encoder::Nvenc => preset.nvenc_cq.to_string(),
        Encoder::Qsv => preset.qsv_quality.to_string(),
        Encoder::Amf => preset.amf_quality.to_string(),
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
    );

    // Use a ListState to handle scrolling automatically
    let mut list_state = ListState::default();
    list_state.select(Some(app.config_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(
                    " {} ({}) ",
                    t(lang, Msg::Settings),
                    AppConfig::config_path().display()
                )),
        )
        .highlight_style(Style::default());

    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Status bar: show save confirmation when present, otherwise show help
    if let Some(ref msg) = app.message {
        let status = Paragraph::new(msg.as_str())
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(status, chunks[2]);
    } else {
        let help_text = if app.config_edit_buffer.is_some() {
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}  ", t(lang, Msg::Confirm))),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}", t(lang, Msg::Cancel))),
            ])
        } else {
            Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}  ", t(lang, Msg::Navigate))),
                Span::styled("←→", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}  ", t(lang, Msg::Adjust))),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}  ", t(lang, Msg::EditText))),
                Span::styled("s", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}  ", t(lang, Msg::Save))),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}  ", t(lang, Msg::Back))),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}", t(lang, Msg::Quit))),
            ])
        };
        let help = Paragraph::new(help_text)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(help, chunks[2]);
    }
}

fn build_config_items(
    config: &AppConfig,
    selected: usize,
    editing: bool,
    input_buffer: &str,
) -> Vec<ListItem<'static>> {
    visible_config_items(config)
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == selected;
            let is_text = item.kind == ConfigItemKind::Text;

            // While editing, show the live buffer with a text cursor
            let display_value = if is_selected && editing && is_text {
                format!("{input_buffer}|")
            } else {
                get_config_value(config, i)
            };

            // Prompt the user how to open the editor for text fields
            let hint = if is_text && is_selected && !editing {
                t(config.language, Msg::EnterToEdit)
            } else {
                ""
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

            let prefix = if is_selected { "> " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}{}: ", prefix, t(config.language, item.label)),
                    label_style,
                ),
                Span::styled(display_value, value_style),
                Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect()
}
