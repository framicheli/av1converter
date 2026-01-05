use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::Stylize,
    widgets::{Block, Borders, Paragraph},
    text::Line,
};

use crate::app::App;

pub fn draw_finish(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .split(size);

    // Title
    let title = Paragraph::new("✓ Encoding Complete!")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL))
        .bold()
        .green();
    frame.render_widget(title, chunks[0]);

    // Completion info
    let mut lines = vec![
        Line::from(""),
        Line::from("Your video has been successfully encoded to AV1!".bold().green()),
        Line::from(""),
    ];

    if let Some(video) = &app.current_video {
        lines.push(Line::from(vec![
            "Input: ".bold(),
            video.filepath.to_string_lossy().to_string().into(),
        ]));
        lines.push(Line::from(""));
    }

    if let Some(start) = app.encode_start_time {
        if let Some(finish) = app.encode_finish_time {
            let total_time = finish.duration_since(start);
            let total_secs = total_time.as_secs();
            let total_min = total_secs / 60;
            let total_sec = total_secs % 60;
            lines.push(Line::from(vec![
                "Total Encoding Time: ".bold(),
                format!("{}:{:02}", total_min, total_sec).into(),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Press Enter or Esc to return to home".gray()));

    let completion_text = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Completion "))
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(completion_text, chunks[1]);

    // Help text
    let help_text = Paragraph::new("Enter/Esc: Return to Home")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title(" Help "))
        .gray();
    frame.render_widget(help_text, chunks[2]);
}

