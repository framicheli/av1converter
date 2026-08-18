use super::common::centered_rect;
use crate::app::{App, ConfirmAction};
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn render_confirm_dialog(f: &mut Frame, app: &App) {
    let Some((action, selected)) = &app.confirm_dialog else {
        return;
    };

    let lang = app.config.language;
    let (title, message) = match action {
        ConfirmAction::CancelEncoding => (
            format!(" {} ", t(lang, Msg::CancelEncodingTitle)),
            t(lang, Msg::CancelEncodingPrompt),
        ),
        ConfirmAction::CancelDisc => (
            format!(" {} ", t(lang, Msg::CancelDiscTitle)),
            t(lang, Msg::CancelDiscPrompt),
        ),
        ConfirmAction::ExitApp => (
            format!(" {} ", t(lang, Msg::ExitAppTitle)),
            t(
                lang,
                if app.work_active() {
                    Msg::ExitAppActivePrompt
                } else {
                    Msg::ExitAppPrompt
                },
            ),
        ),
        ConfirmAction::AbandonTrackConfig => (
            format!(" {} ", t(lang, Msg::AbandonTrackConfigTitle)),
            t(lang, Msg::AbandonTrackConfigPrompt),
        ),
        ConfirmAction::DiscardConfigChanges => (
            format!(" {} ", t(lang, Msg::DiscardConfigTitle)),
            t(lang, Msg::DiscardConfigPrompt),
        ),
        ConfirmAction::CancelAnalysis => (
            format!(" {} ", t(lang, Msg::CancelAnalysisTitle)),
            t(lang, Msg::CancelAnalysisPrompt),
        ),
    };

    // Calculate dialog area (wide/tall enough for longer, wrapped prompts)
    let area = if f.area().width < 50 || f.area().height < 12 {
        f.area()
    } else {
        centered_rect(70, 40, f.area())
    };

    // Clear area behind the dialog
    f.render_widget(Clear, area);

    // Dialog content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
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

    if area.height < 7 {
        let controls = Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Red)),
            Span::raw(format!(" {}  ", t(lang, Msg::Yes))),
            Span::styled("n", Style::default().fg(Color::Green)),
            Span::raw(format!(" {}", t(lang, Msg::No))),
        ]);
        f.render_widget(
            Paragraph::new(controls).alignment(Alignment::Center),
            area.inner(ratatui::layout::Margin {
                horizontal: 1,
                vertical: 1,
            }),
        );
        return;
    }

    // Message
    let msg = Paragraph::new(message)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(msg, chunks[1]);

    // Buttons
    let yes_style = if *selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    let no_style = if *selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let buttons = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(format!(" {} ", t(lang, Msg::Yes)), yes_style),
        Span::raw("    "),
        Span::styled(format!(" {} ", t(lang, Msg::No)), no_style),
        Span::styled("  ", Style::default()),
    ]);

    let buttons_paragraph = Paragraph::new(buttons).alignment(Alignment::Center);
    f.render_widget(buttons_paragraph, chunks[2]);
}
