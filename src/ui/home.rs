use super::common::create_menu_item;
use crate::app::App;
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_home(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(if app.message.is_some() { 3 } else { 0 }),
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

    let lang = app.config.language;

    // Notice area, shown only while a message is set
    if let Some(ref msg) = app.message {
        let message = Paragraph::new(msg.as_str())
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(format!(" {} ", t(lang, Msg::Notice))),
            );
        f.render_widget(message, chunks[1]);
    }

    // Menu
    let menu_area = centered_menu_area(chunks[2]);
    let menu_items: Vec<ListItem> = vec![
        create_menu_item(t(lang, Msg::HomeOpenFile), 0, app.home_index),
        create_menu_item(t(lang, Msg::HomeOpenFolder), 1, app.home_index),
        create_menu_item(t(lang, Msg::HomeOpenFolderRecursive), 2, app.home_index),
        create_menu_item(t(lang, Msg::HomeRipDisc), 3, app.home_index),
        create_menu_item(t(lang, Msg::Configuration), 4, app.home_index),
        create_menu_item(t(lang, Msg::Quit), 5, app.home_index),
    ];

    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {} ", t(lang, Msg::MenuTitle))),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(menu, menu_area);

    // Encoder & dependency status
    let status_info = render_status_info(app);
    let status_widget = Paragraph::new(status_info)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(status_widget, chunks[3]);

    // VMAF Info line
    let vmaf_info = render_vmaf_info(app);
    let vmaf_widget = Paragraph::new(vmaf_info)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(vmaf_widget, chunks[4]);

    // Help
    let help_text = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::Navigate))),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::Select))),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}", t(lang, Msg::Quit))),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[5]);
}

fn render_status_info(app: &App) -> Line<'static> {
    let encoder_span = Span::styled(
        format!(
            "{}: {}",
            t(app.config.language, Msg::EncoderLabel),
            app.config.encoder
        ),
        Style::default().fg(Color::Cyan),
    );

    Line::from(vec![encoder_span])
}

fn render_vmaf_info(app: &App) -> Line<'static> {
    let lang = app.config.language;
    if !app.config.quality.vmaf_enabled {
        return Line::from(vec![Span::styled(
            t(lang, Msg::VmafDisabled),
            Style::default().fg(Color::DarkGray),
        )]);
    }

    // Only libvmaf decides whether the configured verification can run
    if app.deps && app.vmaf_deps {
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw(t(lang, Msg::VmafEnabledOpen)),
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
                t(lang, Msg::DepsNotAvailable),
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

#[cfg(test)]
mod tests {
    use super::render_home;
    use crate::app::App;
    use ratatui::{Terminal, backend::TestBackend};

    fn rendered(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render_home(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    /// The home screen renders `app.message` above the menu.
    #[test]
    fn a_message_reaches_the_home_screen() {
        let mut app = App::new();
        assert!(!rendered(&app).contains("MakeMKV was not found"));

        app.set_message("MakeMKV was not found");
        let screen = rendered(&app);
        assert!(screen.contains("MakeMKV was not found"));
        // The menu still renders below it.
        assert!(screen.contains("AV1 Video Converter"));
    }
}
