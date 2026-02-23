mod analyzer;
mod app;
mod config;
mod encoder;
mod error;
mod queue;
mod tracks;
mod ui;
mod utils;
mod verifier;

use app::{App, ConfirmAction, Screen, TrackFocus};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

use crate::app::HOME_MENU;

fn main() -> io::Result<()> {
    let _log_guard = utils::init_logging();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new();
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        app.process_progress_messages();

        terminal.draw(|f| {
            match app.current_screen.clone() {
                Screen::Home => ui::render_home(f, app),
                Screen::FileExplorer { .. } => ui::render_explorer(f, app),
                Screen::FileConfirm => ui::render_file_confirm(f, app),
                Screen::TrackConfig => ui::render_track_config(f, app),
                Screen::Queue => ui::render_queue(f, app),
                Screen::Finish => ui::render_finish(f, app),
                Screen::Configuration => ui::render_config_screen(f, app),
            }
            if app.confirm_dialog.is_some() {
                ui::render_confirm_dialog(f, app);
            }
        })?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            handle_key(app, key.code);
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, key: KeyCode) {
    if app.confirm_dialog.is_some() {
        handle_confirm_dialog_key(app, key);
        return;
    }

    match &app.current_screen {
        Screen::Home => handle_home_key(app, key),
        Screen::FileExplorer { .. } => handle_explorer_key(app, key),
        Screen::FileConfirm => handle_file_confirm_key(app, key),
        Screen::TrackConfig => handle_track_config_key(app, key),
        Screen::Queue => handle_queue_key(app, key),
        Screen::Finish => handle_finish_key(app, key),
        Screen::Configuration => handle_config_key(app, key),
    }
}

fn handle_confirm_dialog_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(action) = app.confirm_dialog.take() {
                execute_confirm_action(app, action);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.confirm_dialog = None;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
            app.confirm_selection = !app.confirm_selection;
        }
        KeyCode::Enter => {
            if app.confirm_selection {
                if let Some(action) = app.confirm_dialog.take() {
                    execute_confirm_action(app, action);
                }
            } else {
                app.confirm_dialog = None;
            }
        }
        _ => {}
    }
}

fn execute_confirm_action(app: &mut App, action: ConfirmAction) {
    match action {
        ConfirmAction::CancelEncoding => {
            app.cancel_encoding();
        }
        ConfirmAction::ExitApp => {
            app.should_quit = true;
        }
    }
}

fn handle_home_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => {
            app.confirm_dialog = Some(ConfirmAction::ExitApp);
            app.confirm_selection = false;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.home_index > 0 {
                app.home_index -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.home_index < HOME_MENU.len() - 1 {
                app.home_index += 1;
            }
        }
        KeyCode::Enter => match app.home_index {
            0 => app.navigate_to_explorer(false, false), // Open video file
            1 => app.navigate_to_explorer(true, false),  // Open folder
            2 => app.navigate_to_explorer(true, true),   // Open folder recursive
            3 => app.navigate_to_configuration(),        // Configuration
            4 => {
                app.confirm_dialog = Some(ConfirmAction::ExitApp);
                app.confirm_selection = false;
            }
            _ => {}
        },
        _ => {}
    }
}

fn handle_explorer_key(app: &mut App, key: KeyCode) {
    app.clear_message();

    match key {
        KeyCode::Esc => app.navigate_to_home(),
        KeyCode::Up | KeyCode::Char('k') => app.explorer_move_up(),
        KeyCode::Down | KeyCode::Char('j') => app.explorer_move_down(),
        KeyCode::Enter => match app.selection_mode {
            app::SelectionMode::File => app.select_explorer_entry(),
            app::SelectionMode::Folder => app.enter_directory(),
        },
        KeyCode::Char(' ') => match app.selection_mode {
            app::SelectionMode::File => app.toggle_file_selection(),
            app::SelectionMode::Folder => app.select_explorer_entry(),
        },
        _ => {}
    }
}

fn handle_file_confirm_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.cancel_file_confirm(),
        KeyCode::Enter => app.confirm_queued_files(),
        KeyCode::Up | KeyCode::Char('k') => {
            if app.file_confirm_scroll > 0 {
                app.file_confirm_scroll -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.file_confirm_scroll < app.queue.jobs.len().saturating_sub(1) {
                app.file_confirm_scroll += 1;
            }
        }
        _ => {}
    }
}

fn handle_track_config_key(app: &mut App, key: KeyCode) {
    let job = match app.current_config_job() {
        Some(j) => j,
        None => return,
    };

    let audio_count = job.audio_tracks.len();
    let subtitle_count = job.subtitle_tracks.len();

    match key {
        KeyCode::Esc => app.navigate_to_home(),
        KeyCode::Tab => {
            app.track_focus = match app.track_focus {
                TrackFocus::Audio if subtitle_count > 0 => TrackFocus::Subtitle,
                TrackFocus::Audio => TrackFocus::Confirm,
                TrackFocus::Subtitle => TrackFocus::Confirm,
                TrackFocus::Confirm if audio_count > 0 => TrackFocus::Audio,
                TrackFocus::Confirm => TrackFocus::Subtitle,
            };
        }
        KeyCode::Up | KeyCode::Char('k') => match app.track_focus {
            TrackFocus::Audio if app.audio_cursor > 0 => app.audio_cursor -= 1,
            TrackFocus::Subtitle if app.subtitle_cursor > 0 => app.subtitle_cursor -= 1,
            _ => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match app.track_focus {
            TrackFocus::Audio if app.audio_cursor < audio_count.saturating_sub(1) => {
                app.audio_cursor += 1
            }
            TrackFocus::Subtitle if app.subtitle_cursor < subtitle_count.saturating_sub(1) => {
                app.subtitle_cursor += 1
            }
            _ => {}
        },
        KeyCode::Char(' ') => match app.track_focus {
            TrackFocus::Audio => {
                let cursor = app.audio_cursor;
                if let Some(job) = app.current_config_job_mut()
                    && let Some(track) = job.audio_tracks.get(cursor)
                {
                    let idx = track.index;
                    job.track_selection.toggle_audio(idx);
                }
            }
            TrackFocus::Subtitle => {
                let cursor = app.subtitle_cursor;
                if let Some(job) = app.current_config_job_mut()
                    && let Some(track) = job.subtitle_tracks.get(cursor)
                {
                    let idx = track.index;
                    job.track_selection.toggle_subtitle(idx);
                }
            }
            TrackFocus::Confirm => app.confirm_track_config(),
        },
        KeyCode::Char('a') => {
            if let Some(job) = app.current_config_job_mut() {
                let all_indices: Vec<usize> = job.audio_tracks.iter().map(|t| t.index).collect();
                if job.track_selection.audio_indices.len() == all_indices.len() {
                    job.track_selection.audio_indices.clear();
                } else {
                    job.track_selection.audio_indices = all_indices;
                }
            }
        }
        KeyCode::Char('s') => {
            if let Some(job) = app.current_config_job_mut() {
                let all_indices: Vec<usize> = job.subtitle_tracks.iter().map(|t| t.index).collect();
                if job.track_selection.subtitle_indices.len() == all_indices.len() {
                    job.track_selection.subtitle_indices.clear();
                } else {
                    job.track_selection.subtitle_indices = all_indices;
                }
            }
        }
        KeyCode::Enter => app.confirm_track_config(),
        _ => {}
    }
}

fn handle_queue_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc if app.encoding_active => {
            app.confirm_dialog = Some(ConfirmAction::CancelEncoding);
            app.confirm_selection = false;
        }
        KeyCode::Enter if !app.encoding_active => {
            app.navigate_to_finish();
        }
        _ => {}
    }
}

fn handle_finish_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => {
            app.confirm_dialog = Some(ConfirmAction::ExitApp);
            app.confirm_selection = false;
        }
        KeyCode::Enter => app.reset(),
        _ => {}
    }
}

fn handle_config_key(app: &mut App, key: KeyCode) {
    let config_item_count = 10; // Number of config items

    match key {
        KeyCode::Esc => app.navigate_to_home(),
        KeyCode::Up | KeyCode::Char('k') => {
            if app.config_selected > 0 {
                app.config_selected -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.config_selected < config_item_count - 1 {
                app.config_selected += 1;
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            adjust_config_value(app, app.config_selected, false);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            adjust_config_value(app, app.config_selected, true);
        }
        KeyCode::Char('s') => {
            if let Err(e) = app.config.save() {
                tracing::warn!("Failed to save config: {:?}", e);
            }
        }
        _ => {}
    }
}

fn adjust_config_value(app: &mut App, index: usize, increase: bool) {
    match index {
        0 => {
            // Encoder - cycle through options
            use crate::config::Encoder;
            let encoders = [Encoder::SvtAv1, Encoder::Nvenc, Encoder::Qsv, Encoder::Amf];
            let current = encoders
                .iter()
                .position(|e| *e == app.config.encoder)
                .unwrap_or(0);
            let next = if increase {
                (current + 1) % encoders.len()
            } else {
                (current + encoders.len() - 1) % encoders.len()
            };
            app.config.encoder = encoders[next];
        }
        1 => {
            // VMAF Threshold
            let delta = if increase { 1.0 } else { -1.0 };
            app.config.quality.vmaf_threshold =
                (app.config.quality.vmaf_threshold + delta).clamp(0.0, 100.0);
        }
        2 => {
            // VMAF Enabled
            app.config.quality.vmaf_enabled = !app.config.quality.vmaf_enabled;
        }
        3 => {
            // SVT-AV1 Preset
            let delta: i8 = if increase { 1 } else { -1 };
            let new_val = app.config.performance.svt_preset as i8 + delta;
            app.config.performance.svt_preset = new_val.clamp(0, 13) as u8;
        }
        4 => {
            // NVENC Preset - cycle
            let presets = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
            let current = presets
                .iter()
                .position(|p| *p == app.config.performance.nvenc_preset)
                .unwrap_or(6);
            let next = if increase {
                (current + 1) % presets.len()
            } else {
                (current + presets.len() - 1) % presets.len()
            };
            app.config.performance.nvenc_preset = presets[next].to_string();
        }
        7 => {
            // Same Directory Output
            app.config.output.same_directory = !app.config.output.same_directory;
        }
        _ => {} // String fields not adjustable via arrow keys
    }
}
