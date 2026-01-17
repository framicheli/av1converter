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
    let (symbol, color, suffix) = match status {
        FileStatus::Done => ("✓", Color::Green, String::new()),
        FileStatus::Skipped { reason } => ("⊘", Color::Yellow, format!(" ({})", reason)),
        FileStatus::Error { message } => ("✗", Color::Red, format!(": {}", message)),
        _ => ("?", Color::DarkGray, String::new()),
    };

    ListItem::new(format!("  {} {}{}", symbol, name, suffix)).style(Style::default().fg(color))
}
