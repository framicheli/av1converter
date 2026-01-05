use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, Borders, Gauge, Paragraph},
    text::Line,
};

use crate::app::App;

pub fn draw_encode(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(size);

    // Title
    let title = Paragraph::new("Encoding in Progress...")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL))
        .bold()
        .green();
    frame.render_widget(title, chunks[0]);

    // Progress bar
    let progress = app.encode_progress.percentage;
    let progress_text = format!("{:.1}%", progress);
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Progress "))
        .gauge_style(Style::default().fg(Color::Green))
        .percent(progress as u16)
        .label(progress_text);
    frame.render_widget(gauge, chunks[1]);

    // Statistics
    let elapsed = app.encode_progress.elapsed_time;
    let elapsed_secs = elapsed.as_secs();
    let elapsed_min = elapsed_secs / 60;
    let elapsed_sec = elapsed_secs % 60;

    let mut stats_lines = vec![
        Line::from(vec![
            "Elapsed Time: ".bold(),
            format!("{}:{:02}", elapsed_min, elapsed_sec).into(),
        ]),
    ];

    if let Some(eta) = app.encode_progress.estimated_time_remaining {
        let eta_secs = eta.as_secs();
        let eta_min = eta_secs / 60;
        let eta_sec = eta_secs % 60;
        stats_lines.push(Line::from(vec![
            "ETA: ".bold(),
            format!("{}:{:02}", eta_min, eta_sec).into(),
        ]));
    } else {
        stats_lines.push(Line::from("ETA: Calculating...".gray()));
    }

    stats_lines.push(Line::from(vec![
        "Frames Processed: ".bold(),
        app.encode_progress.frame_count.to_string().into(),
    ]));

    let stats = Paragraph::new(stats_lines)
        .block(Block::default().borders(Borders::ALL).title(" Statistics "))
        .alignment(Alignment::Left);
    frame.render_widget(stats, chunks[2]);

    // Video info
    if let Some(video) = &app.current_video {
        let video_info = format!(
            "Encoding: {}",
            video.filepath.file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        let info_para = Paragraph::new(video_info)
            .block(Block::default().borders(Borders::ALL).title(" Current File "))
            .gray();
        frame.render_widget(info_para, chunks[3]);
    }
}

