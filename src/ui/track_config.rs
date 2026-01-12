use crate::app::{App, TrackFocus};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn render_track_config(f: &mut Frame, app: &App) {
    let file = match app.current_config_file() {
        Some(f) => f,
        None => return,
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
                file.filename(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Resolution: ", Style::default().fg(Color::DarkGray)),
            Span::styled(file.resolution_string(), Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                file.hdr_string(),
                Style::default().fg(if file.hdr_string() == "HDR" {
                    Color::Yellow
                } else if file.hdr_string() == "Dolby Vision" {
                    Color::Red
                } else {
                    Color::White
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
    let audio_items: Vec<ListItem> = file
        .audio_tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let selected = file.selected_audio.contains(&track.index);
            let is_cursor = app.track_focus == TrackFocus::Audio && i == app.audio_cursor;
            create_track_item(&track.display_name(), selected, is_cursor)
        })
        .collect();

    let audio_border_color = if app.track_focus == TrackFocus::Audio {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let audio_list = List::new(audio_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(audio_border_color))
            .title(" Audio Tracks [Space to toggle] "),
    );
    f.render_widget(audio_list, track_chunks[0]);

    // Subtitle tracks
    let subtitle_items: Vec<ListItem> = file
        .subtitle_tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let selected = file.selected_subtitles.contains(&track.index);
            let is_cursor = app.track_focus == TrackFocus::Subtitle && i == app.subtitle_cursor;
            create_track_item(&track.display_name(), selected, is_cursor)
        })
        .collect();

    let subtitle_border_color = if app.track_focus == TrackFocus::Subtitle {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let subtitle_list = List::new(subtitle_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(subtitle_border_color))
            .title(" Subtitle Tracks [Space to toggle] "),
    );
    f.render_widget(subtitle_list, track_chunks[1]);

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
