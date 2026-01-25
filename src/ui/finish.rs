use super::home::get_vmaf_color;
use crate::app::App;
use crate::data::FileStatus;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render_finish(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // Summary
    let summary_lines = vec![
        Line::from(vec![Span::styled(
            "Conversion Complete!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw(format!("Converted: {}", app.converted_count)),
            Span::raw("   "),
            Span::styled("⊘ ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("Skipped: {}", app.skipped_count)),
            Span::raw("   "),
            Span::styled("✗ ", Style::default().fg(Color::Red)),
            Span::raw(format!("Errors: {}", app.error_count)),
        ]),
    ];

    let summary = Paragraph::new(summary_lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Summary "),
        );
    f.render_widget(summary, chunks[0]);

    // File list
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|file| create_result_item(&file.filename(), &file.status))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Results "),
    );
    f.render_widget(list, chunks[1]);

    // Help
    let help_text = Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" New conversion  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(" Quit"),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[2]);
}

fn create_result_item(name: &str, status: &FileStatus) -> ListItem<'static> {
    match status {
        FileStatus::Done => {
            ListItem::new(format!("  ✓ {}", name)).style(Style::default().fg(Color::Green))
        }
        FileStatus::DoneWithVmaf { score } => {
            // Color-coded VMAF score
            let vmaf_color = get_vmaf_color(*score);
            let quality_desc = get_quality_description(*score);
            ListItem::new(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(name.to_string()),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {:.1}", score),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({})", quality_desc),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        }
        FileStatus::Skipped { reason } => ListItem::new(format!("  ⊘ {} ({})", name, reason))
            .style(Style::default().fg(Color::Yellow)),
        FileStatus::Error { message } => ListItem::new(format!("  ✗ {}: {}", name, message))
            .style(Style::default().fg(Color::Red)),
        FileStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            ListItem::new(Line::from(vec![
                Span::styled("  ⚠ ", Style::default().fg(Color::Yellow)),
                Span::raw(name.to_string()),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {:.1}", vmaf),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" < {:.0} threshold", threshold),
                    Style::default().fg(Color::Red),
                ),
            ]))
        }
        _ => ListItem::new(format!("  ? {}", name)).style(Style::default().fg(Color::DarkGray)),
    }
}

/// Get quality description for VMAF score
fn get_quality_description(score: f64) -> &'static str {
    match score as u32 {
        95..=100 => "Excellent",
        90..=94 => "Very Good",
        85..=89 => "Good",
        80..=84 => "Fair",
        70..=79 => "Poor",
        _ => "Bad",
    }
}
