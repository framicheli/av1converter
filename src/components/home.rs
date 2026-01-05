use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::App;

pub fn draw_home(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .margin(2)
    .split(size);

    let title = Paragraph::new("AV1 Video Converter")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL))
        .bold()
        .blue();

    frame.render_widget(title, chunks[0]);

    let items = vec![
        ListItem::new("📄 Open video file"),
        ListItem::new("📁 Open folder with multiple video files"),
    ];

    let mut state = ListState::default();
    state.select(Some(app.home_selection));

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Menu "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, chunks[1], &mut state);

    let help_text = Paragraph::new("↑/↓: Navigate  Enter: Select  q: Quit")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title(" Help "))
        .gray();

    frame.render_widget(help_text, chunks[2]);
}
