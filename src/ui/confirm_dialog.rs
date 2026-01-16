use crate::app::{App, ConfirmAction};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render_confirm_dialog(f: &mut Frame, app: &App) {
    let action = match &app.confirm_dialog {
        Some(a) => a,
        None => return,
    };

    let (title, message) = match action {
        ConfirmAction::CancelEncoding => (
            " Cancel Encoding ",
            "Are you sure you want to cancel the current encoding?",
        ),
        ConfirmAction::ExitApp => (" Exit Application ", "Are you sure you want to exit?"),
    };

    // Calculate dialog area
    let area = centered_rect(50, 30, f.area());

    // Clear area behind the dialog
    f.render_widget(Clear, area);

    // Dialog content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(area);

    // Dialog block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title)
        .title_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(block, area);

    // Message
    let msg = Paragraph::new(message)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center);
    f.render_widget(msg, chunks[1]);

    // Buttons
    let yes_style = if app.confirm_selection {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    let no_style = if !app.confirm_selection {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    let buttons = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(" Yes ", yes_style),
        Span::raw("    "),
        Span::styled(" No ", no_style),
        Span::styled("  ", Style::default()),
    ]);

    let buttons_paragraph = Paragraph::new(buttons).alignment(Alignment::Center);
    f.render_widget(buttons_paragraph, chunks[3]);
}

/// Centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
