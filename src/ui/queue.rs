use super::common::{get_vmaf_color, translate_reason};
use crate::app::App;
use crate::i18n::{Language, Msg, t};
use crate::queue::JobStatus;
use crate::utils::format_duration;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
};

#[allow(clippy::too_many_lines)]
pub fn render_queue(f: &mut Frame, app: &mut App) {
    let lang = app.config.language;

    // The detail panel below the list always reflects the job at the cursor,
    // not necessarily the one actively encoding. Give it extra height when
    // showing static status text, since an `Error` can span several lines
    // (ffmpeg's last few stderr lines) — the live gauge only ever needs one.
    let detail_job = app.queue.jobs.get(app.queue_cursor);
    let is_live_gauge = matches!(
        detail_job.map(|j| &j.status),
        Some(JobStatus::Encoding { .. })
    );
    let detail_height = if is_live_gauge { 3 } else { 7 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(detail_height),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // Title with progress header
    let total_to_encode = app.queue.total_jobs_to_encode;

    let title_text = if app.analysis_receiver.is_some() {
        let analyzed = app
            .queue
            .jobs
            .iter()
            .filter(|j| !matches!(j.status, JobStatus::Analyzing))
            .count();
        let total = app.queue.jobs.len();
        format!("{} ({analyzed}/{total})", t(lang, Msg::AnalyzingFilesTitle))
    } else if app.encoding_active {
        if let Some(job) = app.queue.jobs.get(app.queue.current_job_index) {
            if matches!(job.status, JobStatus::Encoding { .. }) {
                let current_number = (app.queue.encoding_progress_done + 1).min(total_to_encode);
                format!(
                    "[{}/{}] {}: {}",
                    current_number,
                    total_to_encode,
                    t(lang, Msg::Encoding),
                    job.filename()
                )
            } else {
                format!(
                    "{} ({}/{})",
                    t(lang, Msg::ConversionQueue),
                    app.queue.encoding_progress_done,
                    total_to_encode
                )
            }
        } else {
            format!("{} (0/{total_to_encode})", t(lang, Msg::ConversionQueue))
        }
    } else {
        let done = app.queue.converted_count + app.queue.skipped_count + app.queue.error_count;
        let total = app.queue.jobs.len();
        format!("{} ({done}/{total})", t(lang, Msg::ConversionQueue))
    };

    let title = Paragraph::new(title_text)
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

    // File list
    let items: Vec<ListItem> = app
        .queue
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let is_current = i == app.queue.current_job_index && app.encoding_active;
            let is_cursor = i == app.queue_cursor;
            create_queue_item(
                &job.filename(),
                &job.status,
                is_current,
                is_cursor,
                job.crf,
                lang,
            )
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" {} ", t(lang, Msg::Files))),
    );
    app.queue_list_state.select(Some(app.queue_cursor));
    f.render_stateful_widget(list, chunks[1], &mut app.queue_list_state);

    // Detail panel for the job at the cursor
    if let Some(job) = detail_job {
        if let JobStatus::Encoding { progress } = &job.status {
            let elapsed_str = app
                .queue
                .elapsed_time()
                .map_or_else(|| "--:--".to_string(), format_duration);

            let eta_str = app
                .queue
                .estimated_time_remaining()
                .map_or_else(|| "--:--".to_string(), format_duration);

            let crf_str = job.crf.map(|c| format!("  CRF: {c}")).unwrap_or_default();

            let label = format!(
                "{progress:.1}%  |  {}: {elapsed_str}  |  {}: {eta_str}{crf_str}",
                t(lang, Msg::Elapsed),
                t(lang, Msg::Eta)
            );

            let gauge = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(format!(" {} ", job.filename())),
                )
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                .ratio(progress.clamp(0.0, 100.0) / 100.0)
                .label(label);
            f.render_widget(gauge, chunks[2]);
        } else {
            let status_text = match &job.status {
                JobStatus::Analyzing => t(lang, Msg::StatusAnalyzing).to_string(),
                JobStatus::AwaitingConfig => t(lang, Msg::StatusConfiguring).to_string(),
                JobStatus::Ready => t(lang, Msg::StatusReady).to_string(),
                JobStatus::Pending => t(lang, Msg::Waiting).to_string(),
                JobStatus::Verifying => t(lang, Msg::StatusVerifying).to_string(),
                JobStatus::Done => t(lang, Msg::Complete).to_string(),
                JobStatus::DoneWithVmaf { score } => {
                    format!("{} — VMAF: {score:.1}", t(lang, Msg::Complete))
                }
                JobStatus::DoneVmafFailed { reason } => {
                    format!("{} (VMAF: {reason})", t(lang, Msg::Complete))
                }
                JobStatus::QualityWarning { vmaf, threshold } => format!(
                    "{}: VMAF {vmaf:.1} < {threshold:.0} {}",
                    t(lang, Msg::QualityWarning),
                    t(lang, Msg::ThresholdLabel)
                ),
                JobStatus::Skipped { reason } => translate_reason(lang, reason),
                JobStatus::Error { message } => message.clone(),
                // Handled by the `if let Encoding` branch above; unreachable here.
                JobStatus::Encoding { .. } => String::new(),
            };
            let status = Paragraph::new(status_text)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(format!(" {} ", t(lang, Msg::Status))),
                );
            f.render_widget(status, chunks[2]);
        }
    }

    // Help
    let help_text = if app.analysis_receiver.is_some() || app.encoding_active {
        Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}  ", t(lang, Msg::Navigate))),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}  ", t(lang, Msg::Cancel))),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}", t(lang, Msg::Quit))),
        ])
    } else {
        Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}  ", t(lang, Msg::Navigate))),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}  ", t(lang, Msg::Continue))),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}", t(lang, Msg::Quit))),
        ])
    };

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[3]);
}

fn create_queue_item(
    name: &str,
    status: &JobStatus,
    is_current: bool,
    is_cursor: bool,
    crf: Option<u8>,
    lang: Language,
) -> ListItem<'static> {
    let bold_mod = if is_current || is_cursor {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };
    let prefix = if is_cursor { "> " } else { "  " };

    let crf_str = crf.map(|c| format!(" [CRF:{c}]")).unwrap_or_default();

    match status {
        JobStatus::Pending => ListItem::new(format!("{prefix}○ {name}"))
            .style(Style::default().fg(Color::DarkGray).add_modifier(bold_mod)),
        JobStatus::Analyzing => ListItem::new(format!(
            "{prefix}◐ {name} {}",
            t(lang, Msg::StatusAnalyzing)
        ))
        .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        JobStatus::AwaitingConfig => ListItem::new(format!(
            "{prefix}◑ {name} {}",
            t(lang, Msg::StatusConfiguring)
        ))
        .style(Style::default().fg(Color::Blue).add_modifier(bold_mod)),
        JobStatus::Ready => {
            ListItem::new(format!("{prefix}● {name} {}", t(lang, Msg::StatusReady)))
                .style(Style::default().fg(Color::Blue).add_modifier(bold_mod))
        }
        JobStatus::Encoding { progress } => {
            ListItem::new(format!("{prefix}▶ {name} {progress:.1}%{crf_str}"))
                .style(Style::default().fg(Color::Cyan).add_modifier(bold_mod))
        }
        JobStatus::Verifying => ListItem::new(format!(
            "{prefix}◈ {name} {}",
            t(lang, Msg::StatusVerifying)
        ))
        .style(Style::default().fg(Color::Cyan).add_modifier(bold_mod)),
        JobStatus::Done => ListItem::new(format!("{prefix}✓ {name} {}", t(lang, Msg::StatusDone)))
            .style(Style::default().fg(Color::Green).add_modifier(bold_mod)),
        JobStatus::DoneVmafFailed { reason } => ListItem::new(Line::from(vec![
            Span::styled(
                format!("{prefix}✓ {name} {} ", t(lang, Msg::StatusDone)),
                Style::default().fg(Color::Green).add_modifier(bold_mod),
            ),
            Span::styled(
                format!("(VMAF: {reason})"),
                Style::default().fg(Color::Yellow).add_modifier(bold_mod),
            ),
        ])),
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{prefix}✓ {name} {} ", t(lang, Msg::StatusDone)),
                    Style::default().fg(Color::Green).add_modifier(bold_mod),
                ),
                Span::styled(
                    format!("VMAF: {score:.1}"),
                    Style::default().fg(vmaf_color).add_modifier(bold_mod),
                ),
            ]))
        }
        JobStatus::Skipped { reason } => ListItem::new(format!(
            "{prefix}⊘ {name} ({})",
            translate_reason(lang, reason)
        ))
        .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        JobStatus::Error { message } => ListItem::new(format!(
            "{prefix}✗ {name} {}: {message}",
            t(lang, Msg::Error)
        ))
        .style(Style::default().fg(Color::Red).add_modifier(bold_mod)),
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{prefix}⚠ {name} "),
                    Style::default().fg(Color::Yellow).add_modifier(bold_mod),
                ),
                Span::styled(
                    format!("VMAF: {vmaf:.1}"),
                    Style::default().fg(vmaf_color).add_modifier(bold_mod),
                ),
                Span::styled(
                    format!(" < {threshold:.0}"),
                    Style::default().fg(Color::Red).add_modifier(bold_mod),
                ),
            ]))
        }
    }
}
