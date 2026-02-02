use super::common::{create_menu_item, get_vmaf_color};
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
        create_menu_item("Open folder (recursive)", 2, app.home_index),
        create_menu_item("Configuration", 3, app.home_index),
        create_menu_item("Quit", 4, app.home_index),
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

    // Encoder & dependency status
    let status_info = render_status_info(app);
    let status_widget = Paragraph::new(status_info)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(status_widget, chunks[2]);

    // VMAF Info line
    let vmaf_info = render_vmaf_info(app);
    let vmaf_widget = Paragraph::new(vmaf_info)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(vmaf_widget, chunks[3]);

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
    f.render_widget(help, chunks[4]);
}

fn render_status_info(app: &App) -> Line<'static> {
    let encoder_span = Span::styled(
        format!("Encoder: {}", app.config.encoder),
        Style::default().fg(Color::Cyan),
    );

    let ab_av1_span = if app.deps.ab_av1 {
        Span::styled("  ab-av1: ✓", Style::default().fg(Color::Green))
    } else {
        Span::styled("  ab-av1: ✗", Style::default().fg(Color::DarkGray))
    };

    Line::from(vec![encoder_span, ab_av1_span])
}

fn render_vmaf_info(app: &App) -> Line<'static> {
    if app.deps.vmaf {
        let _color = get_vmaf_color(app.config.quality.vmaf_threshold);
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw("VMAF quality validation enabled (threshold: "),
            Span::styled(
                format!("{:.0}", app.config.quality.vmaf_threshold),
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

fn centered_menu_area(area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Length(9),
            Constraint::Percentage(20),
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
