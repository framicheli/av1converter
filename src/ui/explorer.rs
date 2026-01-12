use crate::app::{App, SelectionMode};
use crate::data::is_video_file;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::path::PathBuf;

pub fn render_explorer(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // Current path
    let path_text = app.current_dir.to_string_lossy();
    let path = Paragraph::new(path_text.as_ref())
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Current Directory "),
        );
    f.render_widget(path, chunks[0]);

    // File list
    let items: Vec<ListItem> = app
        .dir_entries
        .iter()
        .enumerate()
        .map(|(i, path)| create_entry_item(path, i, app.explorer_index, &app.selection_mode))
        .collect();

    let title = match app.selection_mode {
        SelectionMode::File => " Select Video File ",
        SelectionMode::Folder => " Select Folder ",
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(title),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, chunks[1], &mut app.explorer_list_state);

    // Help
    let help_text = match app.selection_mode {
        SelectionMode::File => Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(" Navigate  "),
            Span::styled("Enter/Space", Style::default().fg(Color::Yellow)),
            Span::raw(" Select  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" Back"),
        ]),
        SelectionMode::Folder => Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(" Navigate  "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" Open folder  "),
            Span::styled("Space", Style::default().fg(Color::Yellow)),
            Span::raw(" Select this folder  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" Back"),
        ]),
    };

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[2]);
}

fn create_entry_item(
    path: &PathBuf,
    index: usize,
    selected: usize,
    mode: &SelectionMode,
) -> ListItem<'static> {
    let is_selected = index == selected;
    let is_parent = path == &PathBuf::from("..");
    let is_dir = path.is_dir() || is_parent;
    let is_video = is_video_file(path);

    let name = if is_parent {
        "..".to_string()
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string())
    };

    let (icon, color) = if is_parent {
        ("↑ ", Color::Yellow)
    } else if is_dir {
        ("▶ ", Color::Blue)
    } else if is_video {
        ("▷ ", Color::Green)
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
        SelectionMode::Folder if is_video => style.add_modifier(Modifier::DIM),
        _ => style,
    };

    let prefix = if is_selected { "> " } else { "  " };
    ListItem::new(format!("{}{}{}", prefix, icon, name)).style(style)
}
