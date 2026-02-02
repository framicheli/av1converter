use super::common::get_vmaf_color;
use crate::app::App;
use crate::queue::JobStatus;
use crate::utils::format_duration;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};

pub fn render_queue(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // Title with progress summary
    let total = app.queue.jobs.len();
    let done = app.queue.converted_count + app.queue.skipped_count + app.queue.error_count;

    let title = Paragraph::new(format!("Conversion Queue ({}/{})", done, total))
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
            create_queue_item(&job.filename(), &job.status, is_current, job.crf)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Files "),
    );
    f.render_widget(list, chunks[1]);

    // Current file progress
    if let Some(job) = app.queue.jobs.get(app.queue.current_job_index) {
        match &job.status {
            JobStatus::Encoding { progress } => {
                let elapsed_str = app
                    .queue
                    .elapsed_time()
                    .map(format_duration)
                    .unwrap_or_else(|| "--:--".to_string());

                let eta_str = app
                    .queue
                    .estimated_time_remaining()
                    .map(format_duration)
                    .unwrap_or_else(|| "--:--".to_string());

                let crf_str = job.crf.map(|c| format!("  CRF: {}", c)).unwrap_or_default();

                let label = format!(
                    "{:.1}%  |  Elapsed: {}  |  ETA: {}{}",
                    progress, elapsed_str, eta_str, crf_str
                );

                let gauge = Gauge::default()
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::DarkGray))
                            .title(format!(" {} ", job.filename())),
                    )
                    .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                    .percent(*progress as u16)
                    .label(label);
                f.render_widget(gauge, chunks[2]);
            }
            JobStatus::SearchingCrf => {
                let status = Paragraph::new("Searching for optimal CRF...")
                    .style(Style::default().fg(Color::Yellow))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow))
                            .title(format!(" {} ", job.filename())),
                    );
                f.render_widget(status, chunks[2]);
            }
            JobStatus::Verifying => {
                let status = Paragraph::new("Verifying output...")
                    .style(Style::default().fg(Color::Cyan))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .title(format!(" {} ", job.filename())),
                    );
                f.render_widget(status, chunks[2]);
            }
            _ => {
                let status_text = match &job.status {
                    JobStatus::Pending => "Waiting...",
                    JobStatus::Done => "Complete!",
                    JobStatus::Skipped { reason } => reason.as_str(),
                    JobStatus::Error { message } => message.as_str(),
                    _ => "",
                };
                let status = Paragraph::new(status_text)
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::DarkGray))
                            .title(" Status "),
                    );
                f.render_widget(status, chunks[2]);
            }
        }
    }

    // Help
    let help_text = if app.encoding_active {
        Line::from(vec![
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" Cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" Continue"),
        ])
    };

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[3]);
}

fn create_queue_item(
    name: &str,
    status: &JobStatus,
    is_current: bool,
    crf: Option<u8>,
) -> ListItem<'static> {
    let bold_mod = if is_current {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    let crf_str = crf.map(|c| format!(" [CRF:{}]", c)).unwrap_or_default();

    match status {
        JobStatus::Pending => ListItem::new(format!("  ○ {}", name))
            .style(Style::default().fg(Color::DarkGray).add_modifier(bold_mod)),
        JobStatus::Analyzing => ListItem::new(format!("  ◐ {} Analyzing...", name))
            .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        JobStatus::AwaitingConfig => ListItem::new(format!("  ◑ {} Configuring...", name))
            .style(Style::default().fg(Color::Blue).add_modifier(bold_mod)),
        JobStatus::Ready => ListItem::new(format!("  ● {} Ready", name))
            .style(Style::default().fg(Color::Blue).add_modifier(bold_mod)),
        JobStatus::SearchingCrf => ListItem::new(format!("  ⟳ {} Searching CRF...", name))
            .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        JobStatus::Encoding { progress } => {
            ListItem::new(format!("  ▶ {} {:.1}%{}", name, progress, crf_str))
                .style(Style::default().fg(Color::Cyan).add_modifier(bold_mod))
        }
        JobStatus::Verifying => ListItem::new(format!("  ◉ {} Verifying...", name))
            .style(Style::default().fg(Color::Cyan).add_modifier(bold_mod)),
        JobStatus::Done => ListItem::new(format!("  ✓ {} Done", name))
            .style(Style::default().fg(Color::Green).add_modifier(bold_mod)),
        JobStatus::DoneWithVmaf { score } => {
            let vmaf_color = get_vmaf_color(*score);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  ✓ {} Done ", name),
                    Style::default().fg(Color::Green).add_modifier(bold_mod),
                ),
                Span::styled(
                    format!("VMAF: {:.1}", score),
                    Style::default().fg(vmaf_color).add_modifier(bold_mod),
                ),
            ]))
        }
        JobStatus::Skipped { reason } => ListItem::new(format!("  ⊘ {} ({})", name, reason))
            .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        JobStatus::Error { message } => ListItem::new(format!("  ✗ {} Error: {}", name, message))
            .style(Style::default().fg(Color::Red).add_modifier(bold_mod)),
        JobStatus::QualityWarning { vmaf, threshold } => {
            let vmaf_color = get_vmaf_color(*vmaf);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  ⚠ {} ", name),
                    Style::default().fg(Color::Yellow).add_modifier(bold_mod),
                ),
                Span::styled(
                    format!("VMAF: {:.1}", vmaf),
                    Style::default().fg(vmaf_color).add_modifier(bold_mod),
                ),
                Span::styled(
                    format!(" < {:.0}", threshold),
                    Style::default().fg(Color::Red).add_modifier(bold_mod),
                ),
            ]))
        }
    }
}
