use super::common::{create_menu_item, message_color, wrapped_rows};
use crate::app::App;
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

pub fn render_home(f: &mut Frame, app: &App) {
    // Notice text width: the frame minus the 2-cell margins and the borders.
    let notice_rows = app.message.as_deref().map_or(0, |msg| {
        wrapped_rows(msg, f.area().width.saturating_sub(6)).min(4) + 2
    });
    // Title, status, VMAF and help rows are three tall when the menu still
    // fits below the notice, one tall otherwise.
    let line_rows = if f.area().height.saturating_sub(4) >= notice_rows + MENU_ROWS + 12 {
        3
    } else {
        1
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(line_rows),
            Constraint::Length(notice_rows),
            Constraint::Min(5),
            Constraint::Length(line_rows),
            Constraint::Length(line_rows),
            Constraint::Length(line_rows),
        ])
        .margin(2)
        .split(f.area());

    let lang = app.config.language;

    // Title
    let title = Paragraph::new(t(lang, Msg::AppTitle))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(title, chunks[0]);

    // Notice area, shown only while a message is set
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

    // Menu
    let labels = [
        t(lang, Msg::HomeOpenFile),
        t(lang, Msg::HomeOpenFolder),
        t(lang, Msg::HomeOpenFolderRecursive),
        t(lang, Msg::HomeRipDisc),
        t(lang, Msg::Configuration),
        t(lang, Msg::Quit),
    ];
    // Widest label plus the "> " prefix and both borders.
    let menu_width = labels
        .iter()
        .map(|label| Line::raw(*label).width() + 4)
        .max()
        .map_or(0, |width| u16::try_from(width).unwrap_or(u16::MAX));
    let menu_area = centered_menu_area(chunks[2], menu_width);
    let menu_items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| create_menu_item(label, index, app.home_index))
        .collect();

    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {} ", t(lang, Msg::MenuTitle))),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    let mut menu_state = ListState::default().with_selected(Some(app.home_index));
    f.render_stateful_widget(menu, menu_area, &mut menu_state);

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
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Select))),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))),
    ]);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[5]);
}

fn render_status_info(app: &App) -> Line<'static> {
    let lang = app.config.language;
    if !app.encoder_deps {
        return Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("{}: {}", t(lang, Msg::EncoderLabel), app.config.encoder),
                Style::default().fg(Color::Yellow),
            ),
        ]);
    }
    let encoder_span = Span::styled(
        format!("{}: {}", t(lang, Msg::EncoderLabel), app.config.encoder),
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

/// Rows of the menu box: six entries and the borders.
const MENU_ROWS: u16 = 8;

/// Half the width of `area`, widened to `min_width` where it fits.
fn centered_menu_area(area: Rect, min_width: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Length(MENU_ROWS + 1),
            Constraint::Percentage(20),
        ])
        .split(area);

    let width = (area.width / 2).max(min_width).min(area.width);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(width),
            Constraint::Fill(1),
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

    #[test]
    fn a_wrapped_notice_shows_every_line() {
        let mut app = App::new();
        app.set_message(&format!("{}LASTWORD", "word ".repeat(20)));

        let mut terminal = Terminal::new(TestBackend::new(50, 30)).unwrap();
        terminal.draw(|f| render_home(f, &app)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();

        assert!(screen.contains("LASTWORD"));
    }

    fn rendered_at(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render_home(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn a_small_terminal_shows_every_menu_entry() {
        let app = App::new();
        let screen = rendered_at(&app, 50, 20);
        for label in [
            "Open video file",
            "Open folder (recursive)",
            "Rip DVD / Blu-ray",
            "Configuration",
            "Quit",
        ] {
            assert!(screen.contains(label), "{label} missing");
        }
    }

    #[test]
    fn a_small_terminal_shows_the_whole_notice_and_the_selected_entry() {
        let mut app = App::new();
        app.home_index = 3;
        app.set_message(
            "MakeMKV was not found. Install it from makemkv.com, or set makemkvcon_path under [disc] in config.toml.",
        );
        let screen = rendered_at(&app, 50, 20);
        assert!(screen.contains("config.toml."));
        assert!(screen.contains("> Rip DVD / Blu-ray"));
    }
}
