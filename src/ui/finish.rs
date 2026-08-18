use super::common::{get_quality_description, get_vmaf_color};
use crate::app::App;
use crate::i18n::{Language, Msg, t};
use crate::queue::JobStatus;
use crate::utils::{format_duration, format_file_size};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_finish(f: &mut Frame, app: &mut App) {
    let is_single_file = app.queue.jobs.len() == 1;

    if is_single_file {
        render_single_file_finish(f, app);
    } else {
        render_multi_file_finish(f, app);
    }
}

#[allow(clippy::too_many_lines)]
fn render_single_file_finish(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .margin(1)
        .split(f.area());

    let lang = app.config.language;
    let Some(job) = app.queue.jobs.first() else {
        return;
    };
    let (heading, heading_color) = match &job.status {
        JobStatus::Error { .. } => (Msg::Error, Color::Red),
        JobStatus::Skipped { .. } => (Msg::Skipped, Color::Yellow),
        JobStatus::DoneVmafFailed { .. } | JobStatus::QualityWarning { .. } => {
            (Msg::QualityWarning, Color::Yellow)
        }
        _ => (Msg::ConversionComplete, Color::Green),
    };
    let elapsed_str = app
        .queue
        .elapsed_time()
        .map(format_duration)
        .unwrap_or_default();

    let mut lines = vec![
        Line::from(vec![Span::styled(
            t(lang, heading),
            Style::default()
                .fg(heading_color)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::FileLabel)),
                Style::default().fg(Color::DarkGray),
            ),
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
                Span::styled(
                    format!("{}: ", t(lang, Msg::Status)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(t(lang, Msg::Success), Style::default().fg(Color::Green)),
            ]));
        }
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            let quality_desc = get_quality_description(lang, *score);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", t(lang, Msg::Status)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(t(lang, Msg::Success), Style::default().fg(Color::Green)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("VMAF: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{score:.1}"),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({quality_desc})"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", t(lang, Msg::Status)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    t(lang, Msg::QualityWarning),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("VMAF: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{vmaf:.1}"),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({}: {threshold:.0})", t(lang, Msg::ThresholdLabel)),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        JobStatus::DoneVmafFailed { reason } => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", t(lang, Msg::Status)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{}: {reason}", t(lang, Msg::QualityWarning)),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
        JobStatus::Error { message } => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", t(lang, Msg::Status)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{}: {message}", t(lang, Msg::Error)),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        JobStatus::Skipped { reason } => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", t(lang, Msg::Status)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(
                        "{}: {}",
                        t(lang, Msg::Skipped),
                        super::common::translate_reason(lang, reason)
                    ),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
        _ => {}
    }

    // Size info
    if let Some(source) = job.source_size {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::SourceLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(format_file_size(source)),
        ]));
    }
    if let Some(output) = job.output_size {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::OutputLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(format_file_size(output)),
        ]));
    }
    if let Some((saved, percent)) = job.size_reduction() {
        let grew = percent < 0.0;
        let amount = if grew {
            job.output_size
                .zip(job.source_size)
                .map_or(0, |(output, source)| output.saturating_sub(source))
        } else {
            saved
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{}: ",
                    t(
                        lang,
                        if grew {
                            Msg::SizeIncrease
                        } else {
                            Msg::ReductionLabel
                        }
                    )
                ),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{} ({:.1}%)", format_file_size(amount), percent.abs()),
                Style::default()
                    .fg(if grew { Color::Red } else { Color::Green })
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Source deletion status
    if job.source_deleted {
        lines.push(Line::from(vec![Span::styled(
            t(lang, Msg::SourceFileDeleted),
            Style::default().fg(Color::Yellow),
        )]));
    } else if let Some(vmaf) = job.source_kept_vmaf {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "{} (VMAF {:.1} < {:.0})",
                t(lang, Msg::SourceKept),
                vmaf,
                app.config.quality.vmaf_threshold
            ),
            Style::default().fg(Color::DarkGray),
        )]));
    }

    if !elapsed_str.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::TimeLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(elapsed_str),
        ]));
    }

    let summary = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .scroll((app.detail_scroll, 0))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {} ", t(lang, Msg::ResultTitle))),
        );
    f.render_widget(summary, chunks[0]);

    // Help
    let help_text = Line::from(vec![
        Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Status))),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::NewConversion))),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[1]);
}

#[allow(clippy::too_many_lines)]
fn render_multi_file_finish(f: &mut Frame, app: &mut App) {
    let lang = app.config.language;
    let detail_height = f.area().height.saturating_sub(17).clamp(5, 12);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(5),
            Constraint::Length(detail_height),
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
    let (heading, heading_color) = if app.queue.error_count > 0 {
        (Msg::Errors, Color::Red)
    } else if app.queue.skipped_count > 0 {
        (Msg::Summary, Color::Yellow)
    } else {
        (Msg::ConversionComplete, Color::Green)
    };

    let mut summary_lines = vec![
        Line::from(vec![Span::styled(
            t(lang, heading),
            Style::default()
                .fg(heading_color)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw(format!(
                "{}: {}",
                t(lang, Msg::Converted),
                app.queue.converted_count
            )),
            Span::raw("   "),
            Span::styled("⊘ ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "{}: {}",
                t(lang, Msg::Skipped),
                app.queue.skipped_count
            )),
            Span::raw("   "),
            Span::styled("✗ ", Style::default().fg(Color::Red)),
            Span::raw(format!(
                "{}: {}",
                t(lang, Msg::Errors),
                app.queue.error_count
            )),
        ]),
    ];

    if total_saved != 0 {
        let grew = total_saved < 0;
        summary_lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{}: ",
                    t(
                        lang,
                        if grew {
                            Msg::TotalSpaceIncreased
                        } else {
                            Msg::TotalSpaceSaved
                        }
                    )
                ),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                saved_str.trim_start_matches('-').to_string(),
                Style::default()
                    .fg(if grew { Color::Red } else { Color::Green })
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if !elapsed_str.is_empty() {
        summary_lines.push(Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::TotalTime)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(elapsed_str),
        ]));
    }

    let summary = Paragraph::new(summary_lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {} ", t(lang, Msg::Summary))),
        );
    f.render_widget(summary, chunks[0]);

    // File list with size reduction
    let items: Vec<ListItem> = app
        .queue
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| create_result_item(job, i == app.finish_cursor, lang))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" {} ", t(lang, Msg::Results))),
    );
    app.finish_list_state.select(Some(app.finish_cursor));
    f.render_stateful_widget(list, chunks[1], &mut app.finish_list_state);

    if let Some(job) = app.queue.jobs.get(app.finish_cursor) {
        let detail = Paragraph::new(result_detail(job, lang))
            .wrap(Wrap { trim: true })
            .scroll((app.detail_scroll, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(format!(" {} ", t(lang, Msg::Status))),
            );
        f.render_widget(detail, chunks[2]);
    }

    // Help
    let help_text = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
        Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Status))),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::NewConversion))),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[3]);
}

fn result_detail(job: &crate::queue::EncodingJob, lang: Language) -> String {
    match &job.status {
        JobStatus::Done => t(lang, Msg::Success).to_string(),
        JobStatus::DoneWithVmaf { score } => format!("VMAF {score:.1}"),
        JobStatus::DoneVmafFailed { reason } => reason.clone(),
        JobStatus::Skipped { reason } => super::common::translate_reason(lang, reason),
        JobStatus::Error { message } => message.clone(),
        JobStatus::QualityWarning { vmaf, threshold } => {
            format!("VMAF {vmaf:.1} < {threshold:.0}")
        }
        _ => t(lang, Msg::Unknown).to_string(),
    }
}

fn create_result_item(
    job: &crate::queue::EncodingJob,
    is_cursor: bool,
    lang: Language,
) -> ListItem<'static> {
    let name = job.filename();
    let prefix = if is_cursor { "> " } else { "  " };
    let bold_mod = if is_cursor {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    // Output size and compression ratio
    let output_info = match (job.output_size, job.size_reduction()) {
        // A negative percentage means the output grew, so the sign is printed
        // rather than assumed.
        (Some(output), Some((_, percent))) => {
            format!(" → {} ({:+.1}%)", format_file_size(output), -percent)
        }
        (Some(output), None) => format!(" → {}", format_file_size(output)),
        _ => String::new(),
    };

    // Source deletion info
    let source_info = if job.source_deleted {
        format!(" [{}]", t(lang, Msg::SourceDeletedTag))
    } else if job.source_kept_vmaf.is_some() {
        format!(" [{}]", t(lang, Msg::SourceKeptTag))
    } else {
        String::new()
    };

    // `ListItem`/`Line::style` replace rather than patch, so `bold_mod` is
    // folded into each arm's single outermost `.style()` call below rather
    // than layered on afterwards.
    match &job.status {
        JobStatus::Done => {
            let mut spans = vec![
                Span::styled(format!("{prefix}✓ "), Style::default().fg(Color::Green)),
                Span::raw(name),
                Span::styled(output_info, Style::default().fg(Color::DarkGray)),
            ];
            if !source_info.is_empty() {
                spans.push(Span::styled(
                    source_info,
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans)).style(Style::default().add_modifier(bold_mod))
        }
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            let quality_desc = get_quality_description(lang, *score);
            let mut spans = vec![
                Span::styled(format!("{prefix}✓ "), Style::default().fg(Color::Green)),
                Span::raw(name),
                Span::styled(output_info, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {score:.1}"),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({quality_desc})"),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if !source_info.is_empty() {
                spans.push(Span::styled(
                    source_info,
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans)).style(Style::default().add_modifier(bold_mod))
        }
        JobStatus::Skipped { reason } => ListItem::new(format!(
            "{prefix}⊘ {name} ({})",
            super::common::translate_reason(lang, reason)
        ))
        .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        JobStatus::Error { .. } => {
            ListItem::new(format!("{prefix}✗ {name}: {}", t(lang, Msg::Error)))
                .style(Style::default().fg(Color::Red).add_modifier(bold_mod))
        }
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            let mut spans = vec![
                Span::styled(format!("{prefix}⚠ "), Style::default().fg(Color::Yellow)),
                Span::raw(name),
                Span::styled(output_info, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("VMAF: {vmaf:.1}"),
                    Style::default().fg(vmaf_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" < {threshold:.0} {}", t(lang, Msg::ThresholdLabel)),
                    Style::default().fg(Color::Red),
                ),
            ];
            if !source_info.is_empty() {
                spans.push(Span::styled(
                    source_info,
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans)).style(Style::default().add_modifier(bold_mod))
        }
        _ => ListItem::new(format!("{prefix}? {name}"))
            .style(Style::default().fg(Color::DarkGray).add_modifier(bold_mod)),
    }
}
