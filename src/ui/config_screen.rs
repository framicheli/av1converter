use crate::app::App;
use crate::config::{AppConfig, Encoder, EncodingPreset};
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
    Encoder,
    VmafThreshold,
    VmafEnabled,
    DeleteSource,
    SvtPreset,
    NvencPreset,
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
    pub label: &'static str,
    pub kind: ConfigItemKind,
    pub field: ConfigField,
}

/// Master list of every config item, in display order.
pub const CONFIG_ITEMS: &[ConfigItem] = &[
    ConfigItem {
        label: "Encoder",
        kind: ConfigItemKind::Cycle,
        field: ConfigField::Encoder,
    },
    ConfigItem {
        label: "VMAF Threshold",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::VmafThreshold,
    },
    ConfigItem {
        label: "VMAF Enabled",
        kind: ConfigItemKind::Toggle,
        field: ConfigField::VmafEnabled,
    },
    ConfigItem {
        label: "Delete Source on Success",
        kind: ConfigItemKind::Toggle,
        field: ConfigField::DeleteSource,
    },
    ConfigItem {
        label: "SVT-AV1 Preset",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::SvtPreset,
    },
    ConfigItem {
        label: "NVENC Preset",
        kind: ConfigItemKind::Cycle,
        field: ConfigField::NvencPreset,
    },
    ConfigItem {
        label: "RF SD",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfSd,
    },
    ConfigItem {
        label: "RF HD (720p)",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfHd,
    },
    ConfigItem {
        label: "RF 1080p SDR",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfFullHd,
    },
    ConfigItem {
        label: "RF 1080p HDR",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfFullHdHdr,
    },
    ConfigItem {
        label: "RF 1080p DV",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfFullHdDv,
    },
    ConfigItem {
        label: "RF 4K SDR",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfUhd,
    },
    ConfigItem {
        label: "RF 4K HDR",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfUhdHdr,
    },
    ConfigItem {
        label: "RF 4K DV",
        kind: ConfigItemKind::Numeric,
        field: ConfigField::RfUhdDv,
    },
    ConfigItem {
        label: "Output Suffix",
        kind: ConfigItemKind::Text,
        field: ConfigField::OutputSuffix,
    },
    ConfigItem {
        label: "Output Container",
        kind: ConfigItemKind::Text,
        field: ConfigField::OutputContainer,
    },
    ConfigItem {
        label: "Same Directory Output",
        kind: ConfigItemKind::Toggle,
        field: ConfigField::SameDirectory,
    },
    ConfigItem {
        label: "Preferred Audio Languages",
        kind: ConfigItemKind::Text,
        field: ConfigField::AudioLanguages,
    },
    ConfigItem {
        label: "Preferred Subtitle Languages",
        kind: ConfigItemKind::Text,
        field: ConfigField::SubtitleLanguages,
    },
];

/// Read the current display value for config item `index` from `config`.
pub fn get_config_value(config: &AppConfig, index: usize) -> String {
    let Some(item) = CONFIG_ITEMS.get(index) else {
        return String::new();
    };
    match item.field {
        ConfigField::Encoder => config.encoder.display_name().to_string(),
        ConfigField::VmafThreshold => format!("{:.0}", config.quality.vmaf_threshold),
        ConfigField::VmafEnabled => bool_display(config.quality.vmaf_enabled),
        ConfigField::DeleteSource => bool_display(config.quality.delete_source_on_success),
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
        ConfigField::SameDirectory => bool_display(config.output.same_directory),
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

fn bool_display(v: bool) -> String {
    if v { "Yes" } else { "No" }.to_string()
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

    // Title
    let title = Paragraph::new("Configuration")
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
                    " Settings (config: {}) ",
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
                Span::raw(" Confirm  "),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(" Cancel"),
            ])
        } else {
            Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                Span::raw(" Navigate  "),
                Span::styled("←→", Style::default().fg(Color::Yellow)),
                Span::raw(" Adjust  "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" Edit text  "),
                Span::styled("s", Style::default().fg(Color::Yellow)),
                Span::raw(" Save  "),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(" Back"),
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
    CONFIG_ITEMS
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
                " (Enter to edit)"
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
                Span::styled(format!("{}{}: ", prefix, item.label), label_style),
                Span::styled(display_value, value_style),
                Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect()
}
