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

pub fn terminal_too_small(screen: crate::app::Screen, area: ratatui::layout::Rect) -> bool {
    let (width, height) = min_size(screen);
    area.width < width || area.height < height
}

fn min_size(screen: crate::app::Screen) -> (u16, u16) {
    match screen {
        crate::app::Screen::Home => (50, 18),
        crate::app::Screen::TrackConfig | crate::app::Screen::Configuration => (80, 24),
        crate::app::Screen::Finish => (70, 24),
        _ => (60, 21),
    }
}

pub fn render_too_small(
    f: &mut ratatui::Frame,
    lang: crate::i18n::Language,
    screen: crate::app::Screen,
) {
    use ratatui::layout::Alignment;
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let (w, h) = min_size(screen);
    let message = crate::i18n::t(lang, crate::i18n::Msg::TerminalTooSmall)
        .replace("{w}", &w.to_string())
        .replace("{h}", &h.to_string());
    let message = Paragraph::new(message)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow))
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(message, f.area());
}

pub fn render_shutting_down(f: &mut ratatui::Frame, lang: crate::i18n::Language) {
    use ratatui::layout::Alignment;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    f.render_widget(
        Paragraph::new(crate::i18n::t(lang, crate::i18n::Msg::ShuttingDown))
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL)),
        f.area(),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        render_confirm_dialog, render_disc_drives, render_dv_dialog, render_explorer, render_queue,
        render_shutting_down, terminal_too_small,
    };
    use crate::analyzer::{HdrType, VideoMetadata};
    use crate::app::{App, ConfirmAction, Entry, MessageKind, Screen, SelectionMode};
    use crate::queue::{EncodingJob, JobStatus};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    fn rendered(width: u16, height: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn compact_terminals_get_the_resize_screen() {
        assert!(terminal_too_small(Screen::Home, Rect::new(0, 0, 40, 14)));
        assert!(!terminal_too_small(Screen::Home, Rect::new(0, 0, 50, 18)));
        assert!(terminal_too_small(
            Screen::TrackConfig,
            Rect::new(0, 0, 60, 21)
        ));
    }

    #[test]
    fn compact_confirmation_keeps_yes_and_no_visible() {
        let mut app = App::new();
        app.confirm_dialog = Some((ConfirmAction::ExitApp, false));

        let screen = rendered(20, 5, |frame| render_confirm_dialog(frame, &app));

        assert!(screen.contains('y'));
        assert!(screen.contains('n'));
    }

    #[test]
    fn profile_five_warning_fits_the_supported_terminal() {
        let mut app = App::new();
        let mut job = EncodingJob::new("profile5.mkv".into());
        job.metadata = Some(VideoMetadata {
            width: 3840,
            height: 2160,
            hdr_type: HdrType::DolbyVision,
            dv_profile: Some(5),
            dv_bl_compat: None,
            hdr10_static: None,
            codec_name: "hevc".to_string(),
            frame_rate_num: 24,
            frame_rate_den: 1,
            duration_secs: 60.0,
        });
        app.queue.jobs.push(job);
        app.dv_dialog = Some(1);

        let screen = rendered(80, 24, |frame| render_dv_dialog(frame, &app));

        assert!(screen.contains("Profile 5"));
        assert!(screen.contains("Use recommended"));
    }

    #[test]
    fn drive_discovery_errors_are_visible_on_the_drive_screen() {
        let mut app = App::new();
        app.message = Some("drive permission denied".to_string());
        app.message_kind = MessageKind::Error;

        let screen = rendered(80, 24, |frame| render_disc_drives(frame, &mut app));

        assert!(screen.contains("drive permission denied"));
    }

    #[test]
    fn disc_images_have_a_scan_action_in_the_explorer() {
        let mut app = App::new();
        app.selection_mode = SelectionMode::DiscFolder;
        app.dir_entries = vec![Entry {
            path: "movie.iso".into(),
            is_dir: false,
            size: Some(1),
        }];

        let screen = rendered(80, 24, |frame| render_explorer(frame, &mut app));

        assert!(screen.contains("Scan disc image"));
    }

    #[test]
    fn analysis_progress_excludes_pending_rips() {
        let mut app = App::new();
        let mut ripping = EncodingJob::new("ripping.mkv".into());
        ripping.status = JobStatus::Ripping { progress: 50.0 };
        let mut analyzing = EncodingJob::new("analyzing.mkv".into());
        analyzing.status = JobStatus::Analyzing;
        let mut analyzed = EncodingJob::new("analyzed.mkv".into());
        analyzed.status = JobStatus::AwaitingConfig;
        app.queue.jobs = vec![ripping, analyzing, analyzed];
        let (_tx, rx) = std::sync::mpsc::channel();
        app.analysis_receiver = Some(rx);
        let (_disc_tx, disc_rx) = std::sync::mpsc::channel();
        app.disc_receiver = Some(disc_rx);

        let screen = rendered(120, 24, |frame| render_queue(frame, &mut app));

        assert!(screen.contains("(1/3)"));
        assert!(screen.contains("Cancel Disc Operation"));
    }

    #[test]
    fn shutdown_replaces_the_previous_screen() {
        let screen = rendered(40, 10, |frame| {
            render_shutting_down(frame, crate::i18n::Language::English);
        });

        assert!(screen.contains("Shutting down"));
    }
}

#[cfg(test)]
mod disc_screen_tests {
    use crate::app::{App, DiscState, Screen};
    use crate::disc::DiscTitle;
    use ratatui::{Terminal, backend::TestBackend};

    fn rendered(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|f| super::render_disc_titles(f, app))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    fn with_titles() -> App {
        let mut app = App::new();
        app.current_screen = Screen::DiscTitles;
        app.disc_state = DiscState::Ready;
        app.disc_titles = vec![DiscTitle {
            id: 1,
            name: "title01.mkv".to_string(),
            duration: std::time::Duration::from_hours(1),
            size_bytes: 24_100_000_000,
            chapters: 12,
            tracks: vec!["eng TrueHD 7.1".to_string()],
        }];
        app
    }

    /// Navigate, Toggle and Rip are listed only when there is a list to act on.
    #[test]
    fn the_help_line_lists_only_usable_keys() {
        let listed = rendered(&mut with_titles());
        assert!(listed.contains("Space"));
        assert!(listed.contains("Enter"));

        let mut scanning = with_titles();
        scanning.disc_state = DiscState::Scanning;
        let screen = rendered(&mut scanning);
        assert!(!screen.contains("Space"), "toggle offered while scanning");
        assert!(!screen.contains("Enter"), "rip offered while scanning");
        assert!(screen.contains("Esc"));

        let mut failed = with_titles();
        failed.disc_state = DiscState::Failed("MakeMKV was not found.".to_string());
        let screen = rendered(&mut failed);
        assert!(!screen.contains("Space"), "toggle offered after a failure");
        assert!(screen.contains("Esc"));
    }

    /// Enter with nothing marked sets a message and starts no rip.
    #[test]
    fn ripping_without_a_selection_says_so() {
        let mut app = with_titles();
        app.start_disc_rip();
        assert!(app.message.is_some());
        assert!(app.disc_receiver.is_none());
    }

    /// Sizes on the title list use the same binary units as the rest of the app.
    #[test]
    fn title_sizes_use_binary_units() {
        assert!(rendered(&mut with_titles()).contains("22.44 GiB"));
    }
}
