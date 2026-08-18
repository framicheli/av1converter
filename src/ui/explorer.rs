use super::common::message_color;
use crate::app::{App, Entry, SelectionMode};
use crate::disc::is_iso;
use crate::i18n::{Msg, t};
use crate::queue::is_video_file;
use crate::utils::format_file_size;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

#[allow(clippy::too_many_lines)]
pub fn render_explorer(f: &mut Frame, app: &mut App) {
    let lang = app.config.language;
    let has_message = app.message.is_some();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_message {
            vec![
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(3),
            ]
        } else {
            vec![
                Constraint::Length(3),
                Constraint::Length(0),
                Constraint::Min(5),
                Constraint::Length(3),
            ]
        })
        .margin(1)
        .split(f.area());

    // Current path (truncated from the start so the current folder, at the
    // end of the path, always stays visible even on narrow terminals)
    let path_text = app.current_dir.to_string_lossy();
    let available_width = chunks[0].width.saturating_sub(2) as usize; // minus borders
    let path = Paragraph::new(truncate_path_start(&path_text, available_width))
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {} ", t(lang, Msg::CurrentDirectory))),
        );
    f.render_widget(path, chunks[0]);

    // Message (if any)
    if let Some(ref msg) = app.message {
        let message = Paragraph::new(msg.as_str())
            .style(Style::default().fg(message_color(app.message_kind)))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(message_color(app.message_kind)))
                    .title(format!(" {} ", t(lang, Msg::Notice))),
            );
        f.render_widget(message, chunks[1]);
    }

    // File list
    let items: Vec<ListItem> = app
        .dir_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_toggled = app.selected_files.contains(&entry.path);
            create_entry_item(
                entry,
                i,
                app.explorer_index,
                &app.selection_mode,
                is_toggled,
            )
        })
        .collect();

    let title = match app.selection_mode {
        SelectionMode::File => t(lang, Msg::SelectVideoFile),
        SelectionMode::Folder | SelectionMode::FolderRecursive => t(lang, Msg::SelectFolder),
        SelectionMode::DiscFolder => t(lang, Msg::DiscSelectFolder),
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {title} ")),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, chunks[2], &mut app.explorer_list_state);

    // Help
    let help_text = match app.selection_mode {
        SelectionMode::File => {
            let mut spans = vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
                Span::styled("Space", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Toggle))),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Proceed))),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Back))),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
            ];
            if !app.selected_files.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    format!(
                        "[{} {}]",
                        app.selected_files.len(),
                        t(lang, Msg::SelectedWord)
                    ),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Line::from(spans)
        }
        SelectionMode::Folder | SelectionMode::FolderRecursive => Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::OpenFolderAction))),
            Span::styled("Space", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::SelectThisFolder))),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Back))),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
        ]),
        SelectionMode::DiscFolder => {
            let image_selected = app
                .dir_entries
                .get(app.explorer_index)
                .is_some_and(|entry| is_iso(&entry.path));
            let enter_action = if image_selected {
                Msg::DiscScanThisImage
            } else {
                Msg::OpenFolderAction
            };
            let scan_action = if image_selected {
                Msg::DiscScanThisImage
            } else {
                Msg::DiscScanThisFolder
            };
            Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, enter_action))),
                Span::styled("Space", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, scan_action))),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Back))),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
            ])
        }
    };

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[3]);
}

/// Fit a path to terminal-cell width, preserving its trailing component.
fn truncate_path_start(path: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if Line::raw(path).width() <= max_width {
        return path.to_string();
    }

    let mut start = path.len();
    for (index, _) in path.char_indices().rev() {
        if Line::raw(&path[index..]).width() + 1 > max_width {
            break;
        }
        start = index;
    }
    format!("…{}", &path[start..])
}

fn create_entry_item(
    entry: &Entry,
    index: usize,
    selected: usize,
    mode: &SelectionMode,
    is_toggled: bool,
) -> ListItem<'static> {
    let is_selected = index == selected;
    let is_parent = entry.is_parent();
    let is_dir = entry.is_dir;
    let is_video = !is_dir && is_video_file(&entry.path);
    let is_disc_image = !is_dir && is_iso(&entry.path);

    let name = if is_parent {
        "..".to_string()
    } else {
        entry.name()
    };

    // Cached file size.
    let metadata_str = entry
        .size
        .filter(|_| is_video)
        .map(|size| format!("  [{}]", format_file_size(size)))
        .unwrap_or_default();

    let (icon, color) = if is_parent {
        ("↑ ", Color::Yellow)
    } else if is_dir {
        ("▶ ", Color::Blue)
    } else if is_toggled {
        ("✓ ", Color::Cyan)
    } else if is_video {
        ("▷ ", Color::Green)
    } else if is_disc_image {
        ("◉ ", Color::Magenta)
    } else {
        ("  ", Color::White)
    };

    let style = if is_selected {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color)
    };

    // Dim non-selectable items in folder mode
    let style = match mode {
        SelectionMode::Folder | SelectionMode::FolderRecursive | SelectionMode::DiscFolder
            if is_video =>
        {
            style.add_modifier(Modifier::DIM)
        }
        _ => style,
    };

    let prefix = if is_selected { "> " } else { "  " };
    ListItem::new(format!("{prefix}{icon}{name}{metadata_str}")).style(style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_truncation_uses_terminal_cell_width() {
        let value = truncate_path_start("/資料/movie", 8);
        assert!(Line::raw(&value).width() <= 8);
        assert_eq!(value, "…/movie");
        assert_eq!(truncate_path_start("/movie", 0), "");
    }
}
