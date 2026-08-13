pub mod common;
pub mod config_screen;
mod confirm_dialog;
mod disc;
mod dv_dialog;
mod explorer;
mod file_confirm;
mod finish;
mod home;
mod queue;
mod track_config;

pub use config_screen::render_config_screen;
pub use confirm_dialog::render_confirm_dialog;
pub use disc::{render_disc_drives, render_disc_titles};
pub use dv_dialog::render_dv_dialog;
pub use explorer::render_explorer;
pub use file_confirm::render_file_confirm;
pub use finish::render_finish;
pub use home::render_home;
pub use queue::render_queue;
pub use track_config::render_track_config;

pub fn terminal_too_small(area: ratatui::layout::Rect) -> bool {
    area.width < 60 || area.height < 21
}

pub fn render_too_small(f: &mut ratatui::Frame, lang: crate::i18n::Language) {
    use ratatui::layout::Alignment;
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let message = Paragraph::new(crate::i18n::t(lang, crate::i18n::Msg::TerminalTooSmall))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow))
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(message, f.area());
}

#[cfg(test)]
mod tests {
    use super::terminal_too_small;
    use ratatui::layout::Rect;

    #[test]
    fn compact_terminals_get_the_resize_screen() {
        assert!(terminal_too_small(Rect::new(0, 0, 40, 14)));
        assert!(!terminal_too_small(Rect::new(0, 0, 60, 21)));
    }
}
