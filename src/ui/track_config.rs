use crate::app::{App, TrackFocus};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render_track_config(f: &mut Frame, app: &mut App) {
    let (filename, resolution_string, hdr_string, audio_data, subtitle_data) = {
        let file = match app.current_config_file() {
            Some(f) => f,
            None => return,
        };

        let audio_data: Vec<(String, bool)> = file
            .audio_tracks
            .iter()
            .map(|track| {
                (
                    track.display_name(),
                    file.selected_audio.contains(&track.index),
                )
            })
            .collect();

        let subtitle_data: Vec<(String, bool)> = file
            .subtitle_tracks
            .iter()
            .map(|track| {
                (
                    track.display_name(),
                    file.selected_subtitles.contains(&track.index),
                )
            })
            .collect();

        (
            file.filename(),
            file.resolution_string(),
            file.hdr_string(),
            audio_data,
            subtitle_data,
        )
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // File info header
    let info_lines = vec![
        Line::from(vec![
            Span::styled("File: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                filename,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Resolution: ", Style::default().fg(Color::DarkGray)),
            Span::styled(resolution_string, Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                match hdr_string {
                    "Dolby Vision" => "Dolby Vision → HDR10".to_string(),
                    _ => hdr_string.to_string(),
                },
                Style::default().fg(match hdr_string {
                    "HDR10" => Color::Yellow,
                    "HLG" => Color::Green,
                    "Dolby Vision" => Color::Magenta,
                    _ => Color::White, // SDR
                }),
            ),
        ]),
    ];

    let info = Paragraph::new(info_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Video Info "),
    );
    f.render_widget(info, chunks[0]);

    // Track selection area
    let track_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Audio tracks
    let audio_items: Vec<ListItem> = audio_data
        .iter()
        .enumerate()
        .map(|(i, (name, selected))| {
            let is_cursor = app.track_focus == TrackFocus::Audio && i == app.audio_cursor;
            create_track_item(name, *selected, is_cursor)
        })
        .collect();

    let audio_border_color = if app.track_focus == TrackFocus::Audio {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let audio_list = List::new(audio_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(audio_border_color))
                .title(" Audio Tracks [Space to toggle] "),
        )
        .highlight_style(Style::default());

    app.audio_list_state.select(Some(app.audio_cursor));
    f.render_stateful_widget(audio_list, track_chunks[0], &mut app.audio_list_state);

    // Subtitle tracks
    let subtitle_items: Vec<ListItem> = subtitle_data
        .iter()
        .enumerate()
        .map(|(i, (name, selected))| {
            let is_cursor = app.track_focus == TrackFocus::Subtitle && i == app.subtitle_cursor;
            create_track_item(name, *selected, is_cursor)
        })
        .collect();

    let subtitle_border_color = if app.track_focus == TrackFocus::Subtitle {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let subtitle_list = List::new(subtitle_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(subtitle_border_color))
                .title(" Subtitle Tracks [Space to toggle] "),
        )
        .highlight_style(Style::default());

    app.subtitle_list_state.select(Some(app.subtitle_cursor));
    f.render_stateful_widget(subtitle_list, track_chunks[1], &mut app.subtitle_list_state);

    // Help / Confirm button
    let confirm_style = if app.track_focus == TrackFocus::Confirm {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let help_text = Line::from(vec![
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" Switch panel  "),
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(" Navigate  "),
        Span::styled("Space", Style::default().fg(Color::Yellow)),
        Span::raw(" Toggle  "),
        Span::styled("a", Style::default().fg(Color::Yellow)),
        Span::raw(" All audio  "),
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(" All subs  "),
        Span::styled(" [", Style::default().fg(Color::DarkGray)),
        Span::styled(" Continue ", confirm_style),
        Span::styled("]", Style::default().fg(Color::DarkGray)),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[2]);
}

fn create_track_item(name: &str, selected: bool, is_cursor: bool) -> ListItem<'static> {
    let checkbox = if selected { "[x]" } else { "[ ]" };
    let prefix = if is_cursor { "> " } else { "  " };

    let style = if is_cursor {
        Style::default().add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    ListItem::new(format!("{}{} {}", prefix, checkbox, name)).style(style)
}
