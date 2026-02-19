use crate::app::App;
use crate::utils::format_file_size;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render_file_confirm(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // Header with total count and size
    let total_size: u64 = app.queue.jobs.iter().filter_map(|j| j.source_size).sum();

    let title_text = format!(
        "{} files selected  ({})",
        app.queue.jobs.len(),
        format_file_size(total_size)
    );

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
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Confirm Selection "),
        );
    f.render_widget(title, chunks[0]);

    // File list
    let items: Vec<ListItem> = app
        .queue
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let size_str = job
                .source_size
                .map(|s| format!("  [{}]", format_file_size(s)))
                .unwrap_or_default();

            let is_highlighted = i == app.file_confirm_scroll;
            let style = if is_highlighted {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };

            let prefix = if is_highlighted { "> " } else { "  " };
            ListItem::new(format!("{}▷ {}{}", prefix, job.filename(), size_str)).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Files "),
    );
    f.render_widget(list, chunks[1]);

    // Help
    let help_text = Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" Proceed  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Back"),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[2]);
}
