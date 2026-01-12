mod analysis;
mod app;
mod converter;
mod data;
mod error;
mod ui;

use app::{App, Screen, TrackFocus};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

fn main() -> io::Result<()> {
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
        // Process any pending encoding progress
        app.process_progress_messages();

        // Draw UI
        terminal.draw(|f| {
            match app.current_screen.clone() {
                Screen::Home => ui::render_home(f, app),
                Screen::FileExplorer { .. } => ui::render_explorer(f, app),
                Screen::TrackConfig => ui::render_track_config(f, app),
                Screen::Queue => ui::render_queue(f, app),
                Screen::Finish => ui::render_finish(f, app),
            }
        })?;

        // Handle input with timeout for progress updates
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code);
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, key: KeyCode) {
    match &app.current_screen {
        Screen::Home => handle_home_key(app, key),
        Screen::FileExplorer { .. } => handle_explorer_key(app, key),
        Screen::TrackConfig => handle_track_config_key(app, key),
        Screen::Queue => handle_queue_key(app, key),
        Screen::Finish => handle_finish_key(app, key),
    }
}

fn handle_home_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Up | KeyCode::Char('k') => {
            if app.home_index > 0 {
                app.home_index -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.home_index < 1 {
                app.home_index += 1;
            }
        }
        KeyCode::Enter => {
            match app.home_index {
                0 => app.navigate_to_explorer(false), // Open file
                1 => app.navigate_to_explorer(true),  // Open folder
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_explorer_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.navigate_to_home(),
        KeyCode::Up | KeyCode::Char('k') => {
            app.explorer_move_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.explorer_move_down();
        }
        KeyCode::Enter => {
            match app.selection_mode {
                app::SelectionMode::File => {
                    // In file mode, Enter selects file or enters directory
                    app.select_explorer_entry();
                }
                app::SelectionMode::Folder => {
                    // In folder mode, Enter navigates into directory
                    app.enter_directory();
                }
            }
        }
        KeyCode::Char(' ') => {
            // Space selects the current item (file or folder depending on mode)
            app.select_explorer_entry();
        }
        _ => {}
    }
}

fn handle_track_config_key(app: &mut App, key: KeyCode) {
    let file = match app.current_config_file() {
        Some(f) => f,
        None => return,
    };

    let audio_count = file.audio_tracks.len();
    let subtitle_count = file.subtitle_tracks.len();

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
        KeyCode::Char(' ') => {
            match app.track_focus {
                TrackFocus::Audio => {
                    let cursor = app.audio_cursor;
                    if let Some(file) = app.current_config_file_mut() {
                        if let Some(track) = file.audio_tracks.get(cursor) {
                            let idx = track.index;
                            file.toggle_audio(idx);
                        }
                    }
                }
                TrackFocus::Subtitle => {
                    let cursor = app.subtitle_cursor;
                    if let Some(file) = app.current_config_file_mut() {
                        if let Some(track) = file.subtitle_tracks.get(cursor) {
                            let idx = track.index;
                            file.toggle_subtitle(idx);
                        }
                    }
                }
                TrackFocus::Confirm => {
                    app.confirm_track_config();
                }
            }
        }
        KeyCode::Char('a') => {
            // Select all audio
            if let Some(file) = app.current_config_file_mut() {
                let all_indices: Vec<usize> = file.audio_tracks.iter().map(|t| t.index).collect();
                if file.selected_audio.len() == all_indices.len() {
                    file.selected_audio.clear();
                } else {
                    file.selected_audio = all_indices;
                }
            }
        }
        KeyCode::Char('s') => {
            // Select all subtitles
            if let Some(file) = app.current_config_file_mut() {
                let all_indices: Vec<usize> =
                    file.subtitle_tracks.iter().map(|t| t.index).collect();
                if file.selected_subtitles.len() == all_indices.len() {
                    file.selected_subtitles.clear();
                } else {
                    file.selected_subtitles = all_indices;
                }
            }
        }
        KeyCode::Enter => {
            app.confirm_track_config();
        }
        _ => {}
    }
}

fn handle_queue_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter if !app.encoding_active => {
            app.navigate_to_finish();
        }
        _ => {}
    }
}

fn handle_finish_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Enter => app.reset(),
        _ => {}
    }
}
