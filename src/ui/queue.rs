use super::home::get_vmaf_color;
use crate::app::{App, format_duration};
use crate::data::FileStatus;
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
    let total = app.files.len();
    let done = app.converted_count + app.skipped_count + app.error_count;

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
        .files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let is_current = i == app.current_file_index && app.encoding_active;
            create_queue_item(&file.filename(), &file.status, is_current)
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
    if let Some(file) = app.files.get(app.current_file_index) {
        if let FileStatus::Encoding { progress } = file.status {
            // Label with elapsed time and ETA
            let elapsed_str = app
                .queue_elapsed_time()
                .map(format_duration)
                .unwrap_or_else(|| "--:--".to_string());

            let eta_str = app
                .queue_estimated_time_remaining()
                .map(format_duration)
                .unwrap_or_else(|| "--:--".to_string());

            let label = format!(
                "{:.1}%  |  Elapsed: {}  |  ETA: {}",
                progress, elapsed_str, eta_str
            );

            let gauge = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(format!(" {} ", file.filename())),
                )
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                .percent(progress as u16)
                .label(label);
            f.render_widget(gauge, chunks[2]);
        } else {
            let status_text = match &file.status {
                FileStatus::Pending => "Waiting...",
                FileStatus::Done => "Complete!",
                FileStatus::Skipped { reason } => reason.as_str(),
                FileStatus::Error { message } => message.as_str(),
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

fn create_queue_item(name: &str, status: &FileStatus, is_current: bool) -> ListItem<'static> {
    let bold_mod = if is_current {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    match status {
        FileStatus::Pending => ListItem::new(format!("  ○ {}", name))
            .style(Style::default().fg(Color::DarkGray).add_modifier(bold_mod)),
        FileStatus::Analyzing => ListItem::new(format!("  ◐ {} Analyzing...", name))
            .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        FileStatus::AwaitingConfig => ListItem::new(format!("  ◑ {} Configuring...", name))
            .style(Style::default().fg(Color::Blue).add_modifier(bold_mod)),
        FileStatus::Ready => ListItem::new(format!("  ● {} Ready", name))
            .style(Style::default().fg(Color::Blue).add_modifier(bold_mod)),
        FileStatus::Encoding { progress } => {
            ListItem::new(format!("  ▶ {} {:.1}%", name, progress))
                .style(Style::default().fg(Color::Cyan).add_modifier(bold_mod))
        }
        FileStatus::Done => ListItem::new(format!("  ✓ {} Done", name))
            .style(Style::default().fg(Color::Green).add_modifier(bold_mod)),
        FileStatus::DoneWithVmaf { score } => {
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
        FileStatus::Skipped { reason } => ListItem::new(format!("  ⊘ {} ({})", name, reason))
            .style(Style::default().fg(Color::Yellow).add_modifier(bold_mod)),
        FileStatus::Error { message } => ListItem::new(format!("  ✗ {} Error: {}", name, message))
            .style(Style::default().fg(Color::Red).add_modifier(bold_mod)),
        FileStatus::QualityWarning { vmaf, threshold } => {
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
