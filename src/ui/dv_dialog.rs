use super::common::centered_rect;
use crate::app::App;
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Modal asking how a Dolby Vision source should be converted:
/// keep the DV metadata (AV1 profile 10) or produce true HDR10.
#[allow(clippy::too_many_lines)]
pub fn render_dv_dialog(f: &mut Frame, app: &App) {
    let Some(selected) = app.dv_dialog else {
        return;
    };

    let lang = app.config.language;
    let (filename, dv_profile) = app.current_config_job().map_or_else(
        || (String::new(), None),
        |job| {
            (
                job.filename(),
                job.metadata.as_ref().and_then(|m| m.dv_profile),
            )
        },
    );
    let is_p5 = dv_profile == Some(5);
    let recommended = usize::from(is_p5);

    let area = centered_rect(76, 60, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(format!(" {} ", t(lang, Msg::DvDialogTitle)))
        .title_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // file + prompt
            Constraint::Length(3), // option 1
            Constraint::Length(3), // option 2
            Constraint::Min(0),    // profile 5 warning
            Constraint::Length(1), // help
        ])
        .margin(2)
        .split(area);

    // File name + prompt
    let profile_str = dv_profile.map_or_else(String::new, |p| format!("  [P{p}]"));
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(filename, Style::default().fg(Color::Cyan)),
            Span::styled(profile_str, Style::default().fg(Color::Magenta)),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            t(lang, Msg::DvDialogPrompt),
            Style::default().fg(Color::White),
        )),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(header, chunks[0]);

    let options = [
        (Msg::DvOptionKeep, Msg::DvOptionKeepDesc),
        (Msg::DvOptionHdr10, Msg::DvOptionHdr10Desc),
    ];

    for (i, (label, desc)) in options.iter().enumerate() {
        let is_selected = selected == i;
        let marker = if is_selected { "> " } else { "  " };
        let rec_tag = if recommended == i {
            format!(" ({})", t(lang, Msg::DvRecommended))
        } else {
            String::new()
        };

        let label_style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let option = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Magenta)),
                Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Yellow)),
                Span::styled(format!(" {} ", t(lang, *label)), label_style),
                Span::styled(rec_tag, Style::default().fg(Color::Green)),
            ]),
            Line::from(Span::styled(
                format!("      {}", t(lang, *desc)),
                Style::default().fg(Color::DarkGray),
            )),
        ]);
        f.render_widget(option, chunks[1 + i]);
    }

    if is_p5 {
        let warning = Paragraph::new(Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                t(lang, Msg::DvP5Warning),
                Style::default().fg(Color::Yellow),
            ),
        ]))
        .wrap(Wrap { trim: true });
        f.render_widget(warning, chunks[3]);
    }

    let help = Line::from(vec![
        Span::styled("↑↓/1-2", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::Select))),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}", t(lang, Msg::Confirm))),
    ]);
    f.render_widget(Paragraph::new(help).alignment(Alignment::Center), chunks[4]);
}
