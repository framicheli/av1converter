use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::ListItem,
};

/// Create a centered rectangle within a given area
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let percent_x = percent_x.min(100);
    let percent_y = percent_y.min(100);
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
    if score >= 95.0 {
        Color::Cyan
    } else if score >= 90.0 {
        Color::Green
    } else if score >= 85.0 {
        Color::Yellow
    } else if score >= 80.0 {
        Color::Rgb(255, 165, 0)
    } else {
        Color::Red
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
    ListItem::new(format!("{prefix}{text}")).style(style)
}

/// Get quality description for VMAF score
pub fn get_quality_description(score: f64) -> &'static str {
    if score >= 95.0 {
        "Excellent"
    } else if score >= 90.0 {
        "Very Good"
    } else if score >= 85.0 {
        "Good"
    } else if score >= 80.0 {
        "Fair"
    } else if score >= 70.0 {
        "Poor"
    } else {
        "Bad"
    }
}
