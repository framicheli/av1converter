use crate::app::{App, CurrentScreen};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

pub fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(25), // Sidebar
            Constraint::Min(0),     // Main Content
        ])
        .split(f.area());

    //  Sidebar
    let items = vec![ListItem::new("Home"), ListItem::new("Bitcoin Config")];

    // Highlight the active one
    let mut state = ListState::default();
    state.select(Some(app.sidebar_index));

    let sidebar = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" PDM "))
        .highlight_style(Style::default().bg(Color::Gray).fg(Color::Black));

    f.render_stateful_widget(sidebar, chunks[0], &mut state);

    // Main Content
    let main_area = chunks[1];

    match app.current_screen {
        CurrentScreen::Home => {
            let text = format!("Welcome to PDM.\n\n(Navigate to 'Bitcoin Config' to load)");
            let p = Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).title(" Home "))
                .wrap(Wrap { trim: true });
            f.render_widget(p, main_area);
        }
        CurrentScreen::FileExplorer => {
            render_file_explorer(f, app, main_area);
        }
        _ => {}
    }
}

fn render_file_explorer(f: &mut Frame, app: &mut App, area: Rect) {
    let files: Vec<ListItem> = app
        .explorer
        .files
        .iter()
        .map(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let display_name = if path.is_dir() {
                format!("📁 {}", name)
            } else {
                format!("📄 {}", name)
            };
            ListItem::new(display_name)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.explorer.selected_index));

    let title = format!(" Select File (Current: {:?}) ", app.explorer.current_dir);

    let list = List::new(files)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, area, &mut state);
}
