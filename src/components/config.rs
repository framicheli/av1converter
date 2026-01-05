use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::App;
use crate::data::QualityPreset;

pub fn draw_config(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(3),
    ])
    .split(size);

    // Title
    let title = Paragraph::new("Configuration")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL))
        .bold()
        .blue();
    frame.render_widget(title, chunks[0]);

    // Main content area
    let main_chunks = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Left: Video information
    draw_video_info(frame, app, main_chunks[0]);

    // Right: Configuration options
    draw_config_options(frame, app, main_chunks[1]);

    // Help text
    let help_text =
        Paragraph::new("↑/↓: Navigate  ←/→/Enter: Change  s: Start encoding  Esc: Back")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" Help "))
            .gray();
    frame.render_widget(help_text, chunks[2]);
}

fn draw_video_info(frame: &mut Frame, app: &App, area: Rect) {
    let video = match &app.current_video {
        Some(v) => v,
        None => {
            let text = Paragraph::new("No video selected")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Video Information "),
                )
                .gray();
            frame.render_widget(text, area);
            return;
        }
    };

    let codec_str = match video.video_codec {
        transcoder::VideoCodec::HEVC => "HEVC",
        transcoder::VideoCodec::H264 => "H.264",
        transcoder::VideoCodec::VP9 => "VP9",
        transcoder::VideoCodec::AV1 => "AV1",
        transcoder::VideoCodec::Unknown => "Unknown",
    };

    let duration_min = (video.duration / 60.0) as u32;
    let duration_sec = (video.duration % 60.0) as u32;
    let resolution_str = video.resolution.to_string();
    let duration_str = format!("{}:{:02}", duration_min, duration_sec);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Filepath: ", Style::default().bold()),
            Span::raw(video.filepath.to_string_lossy()),
        ]),
        Line::from(vec![
            Span::styled("Container: ", Style::default().bold()),
            Span::raw(&video.container),
        ]),
        Line::from(vec![
            Span::styled("Video Codec: ", Style::default().bold()),
            Span::raw(codec_str),
        ]),
        Line::from(vec![
            Span::styled("Resolution: ", Style::default().bold()),
            Span::raw(&resolution_str),
        ]),
        Line::from(vec![
            Span::styled("Duration: ", Style::default().bold()),
            Span::raw(&duration_str),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Audio Tracks: ",
            Style::default().bold(),
        )]),
    ];

    if video.audio_tracks.is_empty() {
        lines.push(Line::from("  (none)").gray());
    } else {
        for track in &video.audio_tracks {
            lines.push(Line::from(format!(
                "  Track {}: {} ({}ch, {}Hz) - {}",
                track.index, track.language, track.channels, track.sample_rate, track.codec
            )));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Video Information "),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn draw_config_options(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(area);

    // Quality
    let quality_value = match app.encode_config.quality {
        QualityPreset::Low => "Low",
        QualityPreset::Medium => "Medium",
        QualityPreset::High => "High",
    };
    let quality_text = format!("Quality: {}", quality_value);
    let is_selected = app.config_selection == 0;
    let quality_style = if is_selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    };
    let quality_para = Paragraph::new(quality_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Quality ")
                .border_style(if is_selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                }),
        )
        .style(quality_style);
    frame.render_widget(quality_para, chunks[0]);
}
