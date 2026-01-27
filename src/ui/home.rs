use crate::app::App;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render_home(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .margin(2)
        .split(f.area());

    // Title
    let title = Paragraph::new("AV1 Video Converter")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(title, chunks[0]);

    // Menu
    let menu_area = centered_menu_area(chunks[1]);
    let menu_items: Vec<ListItem> = vec![
        create_menu_item("Open video file", 0, app.home_index),
        create_menu_item("Open folder", 1, app.home_index),
    ];

    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Menu "),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(menu, menu_area);

    // VMAF Info line (non-interactive)
    let vmaf_info = render_vmaf_info(app);
    let vmaf_widget = Paragraph::new(vmaf_info)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(vmaf_widget, chunks[2]);

    // Help
    let help_text = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(" Navigate  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" Select  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(" Quit"),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help, chunks[3]);
}

fn render_vmaf_info(app: &App) -> Line<'static> {
    if app.config.vmaf_available {
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw("VMAF quality validation enabled (threshold: "),
            Span::styled(
                "90",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(")"),
        ])
    } else {
        Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "VMAF unavailable - FFmpeg not compiled with libvmaf",
                Style::default().fg(Color::Yellow),
            ),
        ])
    }
}

/// Get color for VMAF score/threshold
pub fn get_vmaf_color(score: f64) -> Color {
    match score as u32 {
        95..=100 => Color::Cyan,            // Excellent/Transparent
        90..=94 => Color::Green,            // Very Good
        85..=89 => Color::Yellow,           // Good
        80..=84 => Color::Rgb(255, 165, 0), // Fair (Orange)
        _ => Color::Red,                    // Poor
    }
}

fn create_menu_item(text: &str, index: usize, selected: usize) -> ListItem<'static> {
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

fn centered_menu_area(area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(6),
            Constraint::Percentage(30),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(vertical[1])[1]
}
