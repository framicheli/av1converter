use super::common::{get_quality_description, get_vmaf_color};
use crate::app::App;
use crate::queue::JobStatus;
use crate::utils::{format_duration, format_file_size};
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
            Constraint::Length(7),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // Summary with space saved
    let (total_saved, saved_str) = app.queue.total_space_saved();
    let elapsed_str = app
        .queue
        .elapsed_time()
        .map(format_duration)
        .unwrap_or_default();

    let mut summary_lines = vec![
        Line::from(vec![Span::styled(
            "Conversion Complete!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw(format!("Converted: {}", app.queue.converted_count)),
            Span::raw("   "),
            Span::styled("⊘ ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("Skipped: {}", app.queue.skipped_count)),
            Span::raw("   "),
            Span::styled("✗ ", Style::default().fg(Color::Red)),
            Span::raw(format!("Errors: {}", app.queue.error_count)),
        ]),
    ];

    if total_saved > 0 {
        summary_lines.push(Line::from(vec![
            Span::styled("Total space saved: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                saved_str,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if !elapsed_str.is_empty() {
        summary_lines.push(Line::from(vec![
            Span::styled("Total time: ", Style::default().fg(Color::DarkGray)),
            Span::raw(elapsed_str),
        ]));
    }

    let summary = Paragraph::new(summary_lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Summary "),
        );
    f.render_widget(summary, chunks[0]);

    // File list with size reduction
    let items: Vec<ListItem> = app
        .queue
        .jobs
        .iter()
        .map(|job| create_result_item(job))
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

fn create_result_item(job: &crate::queue::EncodingJob) -> ListItem<'static> {
    let name = job.filename();

    // Size reduction info
    let size_info = job
        .size_reduction()
        .map(|(saved, percent)| format!(" (-{}, {:.1}%)", format_file_size(saved), percent))
        .unwrap_or_default();

    match &job.status {
        JobStatus::Done => ListItem::new(format!("  ✓ {}{}", name, size_info))
            .style(Style::default().fg(Color::Green)),
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            let quality_desc = get_quality_description(*score);
            ListItem::new(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(name),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {:.1}", score),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({})", quality_desc),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(size_info, Style::default().fg(Color::DarkGray)),
            ]))
        }
        JobStatus::Skipped { reason } => ListItem::new(format!("  ⊘ {} ({})", name, reason))
            .style(Style::default().fg(Color::Yellow)),
        JobStatus::Error { message } => ListItem::new(format!("  ✗ {}: {}", name, message))
            .style(Style::default().fg(Color::Red)),
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            ListItem::new(Line::from(vec![
                Span::styled("  ⚠ ", Style::default().fg(Color::Yellow)),
                Span::raw(name),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {:.1}", vmaf),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" < {:.0} threshold", threshold),
                    Style::default().fg(Color::Red),
                ),
                Span::styled(size_info, Style::default().fg(Color::DarkGray)),
            ]))
        }
        _ => ListItem::new(format!("  ? {}", name)).style(Style::default().fg(Color::DarkGray)),
    }
}
