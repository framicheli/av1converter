use super::common::message_color;
use crate::app::{App, DiscState};
use crate::i18n::{Msg, t};
use crate::utils::{format_duration, format_file_size};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

/// Drive selection, with a last row for a disc backup on disk. Reached even
/// when the machine has no drive at all.
pub fn render_disc_drives(f: &mut Frame, app: &mut App) {
    let lang = app.config.language;
    let chunks = layout(f);

    f.render_widget(title_bar(t(lang, Msg::DiscSelectDrive)), chunks[0]);

    if matches!(
        app.disc_state,
        DiscState::Discovering | DiscState::Cancelling
    ) {
        let message = if app.disc_state == DiscState::Discovering {
            Msg::DiscDiscovering
        } else {
            Msg::Cancelling
        };
        f.render_widget(
            Paragraph::new(t(lang, message))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow))
                .block(bordered()),
            chunks[1],
        );
        f.render_widget(Paragraph::new(""), chunks[2]);
        let keys = if app.disc_state == DiscState::Discovering {
            vec![("Esc", t(lang, Msg::Cancel)), ("q", t(lang, Msg::Quit))]
        } else {
            vec![("q", t(lang, Msg::Quit))]
        };
        f.render_widget(help(&keys), chunks[3]);
        return;
    }

    let rows = app
        .disc_drives
        .iter()
        .map(|drive| {
            let label = drive
                .disc_label
                .as_deref()
                .unwrap_or(t(lang, Msg::DiscDriveEmpty));
            format!("{} — {label}", drive.name)
        })
        .chain(std::iter::once(t(lang, Msg::DiscOpenFolder).to_string()));

    let items: Vec<ListItem> = rows
        .enumerate()
        .map(|(i, row)| {
            let prefix = if i == app.disc_drive_cursor {
                "> "
            } else {
                "  "
            };
            ListItem::new(format!("{prefix}{row}")).style(Style::default().add_modifier(
                if i == app.disc_drive_cursor {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                },
            ))
        })
        .collect();

    f.render_stateful_widget(
        List::new(items).block(bordered()),
        chunks[1],
        &mut app.disc_drive_list_state,
    );
    if let Some(message) = app.message.as_deref() {
        f.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::default().fg(message_color(app.message_kind)))
                .wrap(Wrap { trim: true })
                .block(bordered().title(format!(" {} ", t(lang, Msg::Notice)))),
            chunks[2],
        );
    }
    f.render_widget(
        help(&[
            ("↑↓", t(lang, Msg::Navigate)),
            ("Enter", t(lang, Msg::Select)),
            ("Esc", t(lang, Msg::Back)),
            ("q", t(lang, Msg::Quit)),
        ]),
        chunks[3],
    );
}

/// Title selection, with scanning and failure as states of the same screen.
#[allow(clippy::too_many_lines)]
pub fn render_disc_titles(f: &mut Frame, app: &mut App) {
    let lang = app.config.language;
    let chunks = layout(f);

    let disc = app
        .disc_source
        .as_ref()
        .and_then(|source| source.label())
        .unwrap_or(t(lang, Msg::Unknown))
        .to_string();
    f.render_widget(
        title_bar(&format!("{} — {disc}", t(lang, Msg::DiscSelectTitles))),
        chunks[0],
    );

    match &app.disc_state {
        DiscState::Scanning => {
            f.render_widget(
                Paragraph::new(t(lang, Msg::DiscScanning))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Yellow))
                    .block(bordered()),
                chunks[1],
            );
        }
        DiscState::Discovering => {
            f.render_widget(
                Paragraph::new(t(lang, Msg::DiscDiscovering))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Yellow))
                    .block(bordered()),
                chunks[1],
            );
        }
        DiscState::Cancelling => {
            f.render_widget(
                Paragraph::new(t(lang, Msg::Cancelling))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Yellow))
                    .block(bordered()),
                chunks[1],
            );
        }
        DiscState::Failed(message) => {
            f.render_widget(
                Paragraph::new(message.clone())
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Red))
                    .wrap(Wrap { trim: true })
                    .block(bordered()),
                chunks[1],
            );
        }
        DiscState::Ready if app.disc_titles.is_empty() => {
            f.render_widget(
                Paragraph::new(t(lang, Msg::DiscNoTitles))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Yellow))
                    .block(bordered()),
                chunks[1],
            );
        }
        DiscState::Ready => {
            let items: Vec<ListItem> = app
                .disc_titles
                .iter()
                .enumerate()
                .map(|(i, title)| {
                    let selected = app.disc_selected.contains(&title.id);
                    let cursor = i == app.disc_cursor;
                    ListItem::new(format!(
                        "{}{} {} — {}  {}  {} {}",
                        if cursor { "> " } else { "  " },
                        if selected { "[x]" } else { "[ ]" },
                        title.name,
                        format_duration(title.duration),
                        format_file_size(title.size_bytes),
                        title.chapters,
                        t(lang, Msg::DiscChapters),
                    ))
                    .style(
                        Style::default()
                            .fg(if selected { Color::Green } else { Color::Gray })
                            .add_modifier(if cursor {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    )
                })
                .collect();

            f.render_stateful_widget(
                List::new(items).block(bordered()),
                chunks[1],
                &mut app.disc_list_state,
            );
        }
    }

    // Track summaries for the title under the cursor: enough to tell a feature
    // from a commentary angle before spending an hour on it.
    let detail = app.message.clone().unwrap_or_else(|| {
        app.disc_titles
            .get(app.disc_cursor)
            .filter(|_| app.disc_state == DiscState::Ready)
            .map(|title| title.tracks.join("\n"))
            .unwrap_or_default()
    });
    f.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: true })
            .scroll((app.detail_scroll, 0))
            .block(bordered().title(format!(" {} ", t(lang, Msg::VideoInfo)))),
        chunks[2],
    );

    let keys: Vec<(&str, &str)> = if app.disc_state == DiscState::Cancelling {
        vec![("q", t(lang, Msg::Quit))]
    } else if matches!(app.disc_state, DiscState::Ready) && !app.disc_titles.is_empty() {
        vec![
            ("↑↓", t(lang, Msg::Navigate)),
            ("Space", t(lang, Msg::Toggle)),
            ("Enter", t(lang, Msg::DiscRipAction)),
            ("PgUp/PgDn", t(lang, Msg::VideoInfo)),
            ("Esc", t(lang, Msg::Back)),
            ("q", t(lang, Msg::Quit)),
        ]
    } else {
        vec![
            (
                "Esc",
                t(
                    lang,
                    if app.disc_state == DiscState::Scanning {
                        Msg::Cancel
                    } else {
                        Msg::Back
                    },
                ),
            ),
            ("q", t(lang, Msg::Quit)),
        ]
    };
    f.render_widget(help(&keys), chunks[3]);
}

fn layout(f: &Frame) -> std::rc::Rc<[ratatui::layout::Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(7),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area())
}

fn bordered() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
}

fn title_bar(text: &str) -> Paragraph<'static> {
    Paragraph::new(text.to_string())
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(bordered())
}

fn help(keys: &[(&str, &str)]) -> Paragraph<'static> {
    let spans: Vec<Span> = keys
        .iter()
        .flat_map(|(key, label)| {
            [
                Span::styled((*key).to_string(), Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{label}  ")),
            ]
        })
        .collect();
    Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
}
