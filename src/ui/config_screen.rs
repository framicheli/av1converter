use crate::app::App;
use crate::config::AppConfig;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

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
    let items = build_config_items(&app.config, app.config_selected);

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

    // Help
    let help_text = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(" Navigate  "),
        Span::styled("←→", Style::default().fg(Color::Yellow)),
        Span::raw(" Adjust value  "),
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(" Save  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Back"),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[2]);
}

fn build_config_items(config: &AppConfig, selected: usize) -> Vec<ListItem<'static>> {
    let items = vec![
        ("Encoder", config.encoder.display_name().to_string()),
        (
            "VMAF Threshold",
            format!("{:.0}", config.quality.vmaf_threshold),
        ),
        (
            "VMAF Enabled",
            if config.quality.vmaf_enabled {
                "Yes".to_string()
            } else {
                "No".to_string()
            },
        ),
        ("SVT-AV1 Preset", config.performance.svt_preset.to_string()),
        ("NVENC Preset", config.performance.nvenc_preset.clone()),
        ("Output Suffix", config.output.suffix.clone()),
        ("Output Container", config.output.container.clone()),
        (
            "Same Directory Output",
            if config.output.same_directory {
                "Yes".to_string()
            } else {
                "No".to_string()
            },
        ),
        (
            "Preferred Audio Languages",
            config.tracks.preferred_audio_languages.join(", "),
        ),
        (
            "Preferred Subtitle Languages",
            config.tracks.preferred_subtitle_languages.join(", "),
        ),
    ];

    items
        .into_iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let is_selected = i == selected;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_selected { "> " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{}: ", prefix, label), style),
                Span::styled(
                    value,
                    if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
            ]))
        })
        .collect()
}
