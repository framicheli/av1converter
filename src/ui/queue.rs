use crate::app::App;
use crate::data::FileStatus;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
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
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
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
        if let FileStatus::Converting { progress } = file.status {
            let gauge = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(format!(" {} ", file.filename())),
                )
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                .percent(progress as u16)
                .label(format!("{:.1}%", progress));
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
    let (symbol, color, suffix) = match status {
        FileStatus::Pending => ("○", Color::DarkGray, String::new()),
        FileStatus::Analyzing => ("◐", Color::Yellow, " Analyzing...".to_string()),
        FileStatus::AwaitingConfig => ("◑", Color::Blue, " Configuring...".to_string()),
        FileStatus::ReadyToConvert => ("●", Color::Blue, " Ready".to_string()),
        FileStatus::Converting { progress } => {
            ("▶", Color::Cyan, format!(" {:.1}%", progress))
        }
        FileStatus::Done => ("✓", Color::Green, " Done".to_string()),
        FileStatus::Skipped { reason } => ("⊘", Color::Yellow, format!(" ({})", reason)),
        FileStatus::Error { message } => ("✗", Color::Red, format!(" Error: {}", message)),
    };

    let style = if is_current {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color)
    };

    ListItem::new(format!("  {} {}{}", symbol, name, suffix)).style(style)
}
