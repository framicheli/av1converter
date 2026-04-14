use crate::app::App;
use crate::config::AppConfig;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
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

/// Descriptor for a single row in the config screen.
pub struct ConfigItem {
    pub label: &'static str,
    pub kind: ConfigItemKind,
}

/// Master list of every config item, in display order.
pub const CONFIG_ITEMS: &[ConfigItem] = &[
    ConfigItem {
        label: "Encoder",
        kind: ConfigItemKind::Cycle,
    },
    ConfigItem {
        label: "VMAF Threshold",
        kind: ConfigItemKind::Numeric,
    },
    ConfigItem {
        label: "VMAF Enabled",
        kind: ConfigItemKind::Toggle,
    },
    ConfigItem {
        label: "Delete Source on Success",
        kind: ConfigItemKind::Toggle,
    },
    ConfigItem {
        label: "SVT-AV1 Preset",
        kind: ConfigItemKind::Numeric,
    },
    ConfigItem {
        label: "NVENC Preset",
        kind: ConfigItemKind::Cycle,
    },
    ConfigItem {
        label: "Output Suffix",
        kind: ConfigItemKind::Text,
    },
    ConfigItem {
        label: "Output Container",
        kind: ConfigItemKind::Text,
    },
    ConfigItem {
        label: "Same Directory Output",
        kind: ConfigItemKind::Toggle,
    },
    ConfigItem {
        label: "Preferred Audio Languages",
        kind: ConfigItemKind::Text,
    },
    ConfigItem {
        label: "Preferred Subtitle Languages",
        kind: ConfigItemKind::Text,
    },
];

/// Read the current display value for item `index` from `config`.
pub fn get_config_value(config: &AppConfig, index: usize) -> String {
    match index {
        0 => config.encoder.display_name().to_string(),
        1 => format!("{:.0}", config.quality.vmaf_threshold),
        2 => bool_display(config.quality.vmaf_enabled),
        3 => bool_display(config.quality.delete_source_on_success),
        4 => config.performance.svt_preset.to_string(),
        5 => config.performance.nvenc_preset.clone(),
        6 => config.output.suffix.clone(),
        7 => config.output.container.clone(),
        8 => bool_display(config.output.same_directory),
        9 => config.tracks.preferred_audio_languages.join(", "),
        10 => config.tracks.preferred_subtitle_languages.join(", "),
        _ => String::new(),
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
        app.config_editing,
        &app.config_input_buffer,
    );

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(
                " Settings (config: {}) ",
                AppConfig::config_path().display()
            )),
    );
    f.render_widget(list, chunks[1]);

    // Help line changes contextually when a text field is being edited
    let help_text = if app.config_editing {
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
                format!("{}|", input_buffer)
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
