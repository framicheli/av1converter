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
    let is_single_file = app.queue.jobs.len() == 1;

    if is_single_file {
        render_single_file_finish(f, app);
    } else {
        render_multi_file_finish(f, app);
    }
}

fn render_single_file_finish(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .margin(1)
        .split(f.area());

    let job = &app.queue.jobs[0];
    let elapsed_str = app
        .queue
        .elapsed_time()
        .map(format_duration)
        .unwrap_or_default();

    let mut lines = vec![
        Line::from(vec![Span::styled(
            "Conversion Complete!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("File: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                job.filename(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    // Status
    match &job.status {
        JobStatus::Done => {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled("Success", Style::default().fg(Color::Green)),
            ]));
        }
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            let quality_desc = get_quality_description(*score);
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled("Success", Style::default().fg(Color::Green)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("VMAF: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.1}", score),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({})", quality_desc),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled("Quality Warning", Style::default().fg(Color::Yellow)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("VMAF: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.1}", vmaf),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" (threshold: {:.0})", threshold),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        JobStatus::Error { message } => {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("Error: {}", message),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        JobStatus::Skipped { reason } => {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("Skipped: {}", reason),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
        _ => {}
    }

    // Size info
    if let Some(source) = job.source_size {
        lines.push(Line::from(vec![
            Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_file_size(source)),
        ]));
    }
    if let Some(output) = job.output_size {
        lines.push(Line::from(vec![
            Span::styled("Output: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_file_size(output)),
        ]));
    }
    if let Some((saved, percent)) = job.size_reduction() {
        lines.push(Line::from(vec![
            Span::styled("Reduction: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} ({:.1}%)", format_file_size(saved), percent),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Source deletion status
    if job.source_deleted {
        lines.push(Line::from(vec![Span::styled(
            "Source file deleted",
            Style::default().fg(Color::Yellow),
        )]));
    } else if let Some(vmaf) = job.source_kept_vmaf {
        lines.push(Line::from(vec![Span::styled(
            format!("Source kept (VMAF {:.1} < 90)", vmaf),
            Style::default().fg(Color::DarkGray),
        )]));
    }

    if !elapsed_str.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Time: ", Style::default().fg(Color::DarkGray)),
            Span::raw(elapsed_str),
        ]));
    }

    let summary = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Result "),
    );
    f.render_widget(summary, chunks[0]);

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
    f.render_widget(help, chunks[1]);
}

fn render_multi_file_finish(f: &mut Frame, app: &App) {
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

    // Output size and compression ratio
    let output_info = match (job.output_size, job.size_reduction()) {
        (Some(output), Some((_, percent))) => {
            format!(" → {} (-{:.1}%)", format_file_size(output), percent)
        }
        (Some(output), None) => format!(" → {}", format_file_size(output)),
        _ => String::new(),
    };

    // Source deletion info
    let source_info = if job.source_deleted {
        " [source deleted]"
    } else if job.source_kept_vmaf.is_some() {
        " [source kept]"
    } else {
        ""
    };

    match &job.status {
        JobStatus::Done => {
            let mut spans = vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(name),
                Span::styled(output_info, Style::default().fg(Color::DarkGray)),
            ];
            if !source_info.is_empty() {
                spans.push(Span::styled(
                    source_info.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans))
        }
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            let quality_desc = get_quality_description(*score);
            let mut spans = vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::raw(name),
                Span::styled(output_info, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {:.1}", score),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({})", quality_desc),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if !source_info.is_empty() {
                spans.push(Span::styled(
                    source_info.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans))
        }
        JobStatus::Skipped { reason } => ListItem::new(format!("  ⊘ {} ({})", name, reason))
            .style(Style::default().fg(Color::Yellow)),
        JobStatus::Error { message } => ListItem::new(format!("  ✗ {}: {}", name, message))
            .style(Style::default().fg(Color::Red)),
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            let mut spans = vec![
                Span::styled("  ⚠ ", Style::default().fg(Color::Yellow)),
                Span::raw(name),
                Span::styled(output_info, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {:.1}", vmaf),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" < {:.0} threshold", threshold),
                    Style::default().fg(Color::Red),
                ),
            ];
            if !source_info.is_empty() {
                spans.push(Span::styled(
                    source_info.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans))
        }
        _ => ListItem::new(format!("  ? {}", name)).style(Style::default().fg(Color::DarkGray)),
    }
}
