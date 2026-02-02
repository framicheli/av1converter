use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::ListItem,
};

/// Create a centered rectangle within a given area
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

/// Get color for VMAF score/threshold
pub fn get_vmaf_color(score: f64) -> Color {
    match score as u32 {
        95..=100 => Color::Cyan,
        90..=94 => Color::Green,
        85..=89 => Color::Yellow,
        80..=84 => Color::Rgb(255, 165, 0),
        _ => Color::Red,
    }
}

/// Create a menu item with selection styling
pub fn create_menu_item(text: &str, index: usize, selected: usize) -> ListItem<'static> {
    let style = if index == selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let prefix = if index == selected { "> " } else { "  " };
    ListItem::new(format!("{}{}", prefix, text)).style(style)
}

/// Get quality description for VMAF score
pub fn get_quality_description(score: f64) -> &'static str {
    match score as u32 {
        95..=100 => "Excellent",
        90..=94 => "Very Good",
        85..=89 => "Good",
        80..=84 => "Fair",
        70..=79 => "Poor",
        _ => "Bad",
    }
}
