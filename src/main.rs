mod analyzer;
mod app;
mod config;
mod daemon;
mod encoder;
mod error;
mod i18n;
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
use ratatui::{Terminal, backend::CrosstermBackend, widgets::Clear};
use std::io;
use std::time::Duration;

use crate::app::HOME_MENU;
use crate::i18n::{Msg, t};

const USAGE: &str = "\
Usage: av1converter [OPTION]

  (no option)   start the interactive TUI
  --daemon      run headless with the web UI (must be enabled in Settings)
  --help        show this help
  --version     show the version
";

enum Cli {
    Tui,
    Daemon,
    Help,
    Version,
    Unknown(String),
}

fn parse_cli() -> Cli {
    match std::env::args().nth(1).as_deref() {
        None => Cli::Tui,
        Some("--daemon") => Cli::Daemon,
        Some("--help" | "-h") => Cli::Help,
        Some("--version" | "-V") => Cli::Version,
        Some(other) => Cli::Unknown(other.to_string()),
    }
}

/// Headless daemon entry: refuses to start unless enabled in the config.
fn run_daemon_entry() -> io::Result<()> {
    let config = config::AppConfig::load();
    if !config.daemon.enabled {
        eprintln!("{}", t(config.language, Msg::DaemonDisabledError));
        std::process::exit(1);
    }
    utils::init_daemon_logging();
    daemon::run_daemon(config).map_err(io::Error::other)
}

fn main() -> io::Result<()> {
    match parse_cli() {
        Cli::Tui => {}
        Cli::Daemon => return run_daemon_entry(),
        Cli::Help => {
            print!("{USAGE}");
            return Ok(());
        }
        Cli::Version => {
            println!("av1converter {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Cli::Unknown(arg) => {
            eprintln!("Unknown argument: {arg}\n{USAGE}");
            std::process::exit(2);
        }
    }

    let _log_guard = utils::init_logging();

    // Restore the terminal even if panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

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
        eprintln!("Error: {err:?}");
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        app.process_progress_messages();
        app.process_analysis_messages();
        app.tick_message();

        terminal.draw(|f| {
            f.render_widget(Clear, f.area());
            match app.current_screen {
                Screen::Home => ui::render_home(f, app),
                Screen::FileExplorer { .. } => ui::render_explorer(f, app),
                Screen::FileConfirm => ui::render_file_confirm(f, app),
                Screen::TrackConfig => ui::render_track_config(f, app),
                Screen::Queue => ui::render_queue(f, app),
                Screen::Finish => ui::render_finish(f, app),
                Screen::Configuration => ui::render_config_screen(f, app),
            }
            if app.dv_dialog.is_some() && app.current_screen == Screen::TrackConfig {
                ui::render_dv_dialog(f, app);
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

    if app.dv_dialog.is_some() && app.current_screen == Screen::TrackConfig {
        handle_dv_dialog_key(app, key);
        return;
    }

    // Global quit shortcut, available on every screen. Suppressed while typing
    // in a Configuration text field so 'q' can still be entered as a character.
    if key == KeyCode::Char('q') && app.config_edit_buffer.is_none() {
        app.confirm_dialog = Some((ConfirmAction::ExitApp, false));
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
        KeyCode::Char('y' | 'Y') => {
            if let Some((action, _)) = app.confirm_dialog.take() {
                execute_confirm_action(app, action);
            }
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.confirm_dialog = None;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') => {
            if let Some((_, sel)) = &mut app.confirm_dialog {
                *sel = !*sel;
            }
        }
        KeyCode::Enter => {
            if let Some((action, true)) = app.confirm_dialog.take() {
                execute_confirm_action(app, action);
            }
        }
        _ => {}
    }
}

fn handle_dv_dialog_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Tab
        | KeyCode::Char('h' | 'j' | 'k' | 'l') => {
            if let Some(sel) = &mut app.dv_dialog {
                *sel = 1 - *sel;
            }
        }
        KeyCode::Char('1') => {
            app.dv_dialog = Some(0);
            app.confirm_dv_dialog();
        }
        KeyCode::Char('2') => {
            app.dv_dialog = Some(1);
            app.confirm_dv_dialog();
        }
        KeyCode::Enter | KeyCode::Char(' ') => app.confirm_dv_dialog(),
        KeyCode::Esc => app.dismiss_dv_dialog(),
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
        ConfirmAction::AbandonTrackConfig => {
            app.cancel_track_config();
        }
        ConfirmAction::DiscardConfigChanges => {
            if let Some(snapshot) = app.config_snapshot.take() {
                app.config = snapshot;
            }
            app.navigate_to_home();
        }
        ConfirmAction::CancelAnalysis => {
            app.cancel_analysis();
        }
    }
}

fn handle_home_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up | KeyCode::Char('k') if app.home_index > 0 => app.home_index -= 1,
        KeyCode::Down | KeyCode::Char('j') if app.home_index < HOME_MENU.len() - 1 => {
            app.home_index += 1;
        }
        KeyCode::Enter => match app.home_index {
            0 => app.navigate_to_explorer(false, false), // Open video file
            1 => app.navigate_to_explorer(true, false),  // Open folder
            2 => app.navigate_to_explorer(true, true),   // Open folder recursive
            3 => app.navigate_to_configuration(),        // Configuration
            4 => {
                app.confirm_dialog = Some((ConfirmAction::ExitApp, false));
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
            app::SelectionMode::Folder | app::SelectionMode::FolderRecursive => {
                app.enter_directory();
            }
        },
        KeyCode::Char(' ') => match app.selection_mode {
            app::SelectionMode::File => app.toggle_file_selection(),
            app::SelectionMode::Folder | app::SelectionMode::FolderRecursive => {
                app.select_explorer_entry();
            }
        },
        _ => {}
    }
}

fn handle_file_confirm_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.cancel_file_confirm(),
        KeyCode::Enter => app.confirm_queued_files(),
        KeyCode::Up | KeyCode::Char('k') if app.file_confirm_scroll > 0 => {
            app.file_confirm_scroll -= 1;
        }
        KeyCode::Down | KeyCode::Char('j')
            if app.file_confirm_scroll < app.queue.jobs.len().saturating_sub(1) =>
        {
            app.file_confirm_scroll += 1;
        }
        _ => {}
    }
}

fn handle_track_config_key(app: &mut App, key: KeyCode) {
    let Some(job) = app.current_config_job() else {
        return;
    };

    let audio_count = job.audio_tracks.len();
    let subtitle_count = job.subtitle_tracks.len();

    match key {
        KeyCode::Esc => {
            app.confirm_dialog = Some((ConfirmAction::AbandonTrackConfig, false));
        }
        KeyCode::Left | KeyCode::Char('h') => app.step_track_config_job(false),
        KeyCode::Right | KeyCode::Char('l') => app.step_track_config_job(true),
        KeyCode::Tab => {
            app.track_focus = match app.track_focus {
                TrackFocus::Confirm if audio_count > 0 => TrackFocus::Audio,
                TrackFocus::Audio | TrackFocus::Confirm if subtitle_count > 0 => {
                    TrackFocus::Subtitle
                }
                TrackFocus::Audio | TrackFocus::Subtitle | TrackFocus::Confirm => {
                    TrackFocus::Confirm
                }
            };
        }
        KeyCode::Up | KeyCode::Char('k') => match app.track_focus {
            TrackFocus::Audio if app.audio_cursor > 0 => app.audio_cursor -= 1,
            TrackFocus::Subtitle if app.subtitle_cursor > 0 => app.subtitle_cursor -= 1,
            _ => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match app.track_focus {
            TrackFocus::Audio if app.audio_cursor < audio_count.saturating_sub(1) => {
                app.audio_cursor += 1;
            }
            TrackFocus::Subtitle if app.subtitle_cursor < subtitle_count.saturating_sub(1) => {
                app.subtitle_cursor += 1;
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
        KeyCode::Char('r' | 'R') => {
            let output_config = app.config.output.clone();
            if let Some(job) = app.current_config_job_mut() {
                job.remux_only = !job.remux_only;
                job.generate_output_path(&output_config);
            }
            // Switching a DV job from remux to encode needs a DV decision
            app.maybe_open_dv_dialog();
        }
        KeyCode::Char('d' | 'D') => app.reopen_dv_dialog(),
        KeyCode::Enter => app.confirm_track_config(),
        _ => {}
    }
}

fn handle_queue_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc if app.analysis_receiver.is_some() => {
            app.confirm_dialog = Some((ConfirmAction::CancelAnalysis, false));
        }
        KeyCode::Esc if app.encoding_active => {
            app.confirm_dialog = Some((ConfirmAction::CancelEncoding, false));
        }
        KeyCode::Up | KeyCode::Char('k') => app.queue_move_cursor(false),
        KeyCode::Down | KeyCode::Char('j') => app.queue_move_cursor(true),
        KeyCode::Enter if !app.encoding_active && app.analysis_receiver.is_none() => {
            app.navigate_to_finish();
        }
        _ => {}
    }
}

fn handle_finish_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up | KeyCode::Char('k') => app.finish_move_cursor(false),
        KeyCode::Down | KeyCode::Char('j') => app.finish_move_cursor(true),
        KeyCode::Enter => app.reset(),
        _ => {}
    }
}

fn handle_config_key(app: &mut App, key: KeyCode) {
    if app.config_edit_buffer.is_some() {
        match key {
            KeyCode::Enter => commit_config_edit(app),
            KeyCode::Esc => {
                app.config_edit_buffer = None;
            }
            KeyCode::Backspace => {
                if let Some(buf) = &mut app.config_edit_buffer {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(buf) = &mut app.config_edit_buffer {
                    buf.push(c);
                }
            }
            _ => {}
        }
        return;
    }

    let config_item_count = crate::ui::config_screen::visible_config_items(&app.config).len();

    match key {
        KeyCode::Esc => {
            if app.config_is_dirty() {
                app.confirm_dialog = Some((ConfirmAction::DiscardConfigChanges, false));
            } else {
                app.navigate_to_home();
            }
        }
        KeyCode::Up | KeyCode::Char('k') if app.config_selected > 0 => {
            app.config_selected -= 1;
        }
        KeyCode::Down | KeyCode::Char('j') if app.config_selected < config_item_count - 1 => {
            app.config_selected += 1;
        }
        KeyCode::Left | KeyCode::Char('h') => {
            adjust_config_value(app, app.config_selected, false);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            adjust_config_value(app, app.config_selected, true);
        }
        KeyCode::Enter => {
            use crate::ui::config_screen::{ConfigItemKind, visible_config_items};
            if visible_config_items(&app.config)
                .get(app.config_selected)
                .is_some_and(|item| item.kind == ConfigItemKind::Text)
            {
                start_config_edit(app);
            }
        }
        KeyCode::Char('s') => {
            let lang = app.config.language;
            if let Err(e) = app.config.save() {
                tracing::warn!("Failed to save config: {:?}", e);
                app.set_timed_message(&format!("{}: {e}", t(lang, Msg::SaveFailed)), 3);
            } else {
                app.config_snapshot = Some(app.config.clone());
                app.set_timed_message(t(lang, Msg::SavedExclaim), 3);
            }
        }
        _ => {}
    }
}

/// Begin editing the currently selected text-editable config field.
fn start_config_edit(app: &mut App) {
    use crate::ui::config_screen::{ConfigField, visible_config_items};
    let Some(item) = visible_config_items(&app.config)
        .get(app.config_selected)
        .copied()
    else {
        return;
    };
    app.config_edit_buffer = Some(match item.field {
        ConfigField::OutputSuffix => app.config.output.suffix.clone(),
        ConfigField::OutputContainer => app.config.output.container.clone(),
        ConfigField::AudioLanguages => app.config.tracks.preferred_audio_languages.join(", "),
        ConfigField::SubtitleLanguages => app.config.tracks.preferred_subtitle_languages.join(", "),
        ConfigField::DaemonBindAddress => app.config.daemon.bind_address.clone(),
        ConfigField::DaemonPort => app.config.daemon.port.to_string(),
        _ => return,
    });
}

/// Write the edit buffer back to the appropriate config field.
fn commit_config_edit(app: &mut App) {
    use crate::ui::config_screen::{ConfigField, visible_config_items};
    let Some(buf) = app.config_edit_buffer.take() else {
        return;
    };
    let Some(item) = visible_config_items(&app.config)
        .get(app.config_selected)
        .copied()
    else {
        return;
    };
    let value = buf.trim().to_string();
    match item.field {
        ConfigField::OutputSuffix => app.config.output.suffix = value,
        ConfigField::OutputContainer => app.config.output.container = value,
        ConfigField::AudioLanguages => {
            app.config.tracks.preferred_audio_languages = parse_lang_list(&value);
        }
        ConfigField::SubtitleLanguages => {
            app.config.tracks.preferred_subtitle_languages = parse_lang_list(&value);
        }
        // Invalid addresses/ports keep the previous value
        ConfigField::DaemonBindAddress => {
            if value.parse::<std::net::IpAddr>().is_ok() {
                app.config.daemon.bind_address = value;
            }
        }
        ConfigField::DaemonPort => {
            if let Ok(port) = value.parse::<u16>()
                && port != 0
            {
                app.config.daemon.port = port;
            }
        }
        _ => {}
    }
}

/// Parse a comma-separated language tag list like `"eng, ita"` into `["eng", "ita"]`.
fn parse_lang_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn adjust_config_value(app: &mut App, index: usize, increase: bool) {
    use crate::ui::config_screen::{ConfigField, visible_config_items};

    let Some(field) = visible_config_items(&app.config)
        .get(index)
        .map(|item| item.field)
    else {
        return;
    };

    match field {
        ConfigField::Language => {
            app.config.language = if increase {
                app.config.language.next()
            } else {
                app.config.language.prev()
            };
        }
        ConfigField::Encoder => {
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
        ConfigField::VmafThreshold => {
            let delta = if increase { 1.0 } else { -1.0 };
            app.config.quality.vmaf_threshold =
                (app.config.quality.vmaf_threshold + delta).clamp(0.0, 100.0);
        }
        ConfigField::VmafEnabled => {
            app.config.quality.vmaf_enabled = !app.config.quality.vmaf_enabled;
        }
        ConfigField::DeleteSource => {
            app.config.quality.delete_source_on_success =
                !app.config.quality.delete_source_on_success;
        }
        ConfigField::SvtPreset => {
            if increase {
                app.config.performance.svt_preset =
                    app.config.performance.svt_preset.saturating_add(1).min(13);
            } else {
                app.config.performance.svt_preset =
                    app.config.performance.svt_preset.saturating_sub(1);
            }
        }
        ConfigField::NvencPreset => {
            let presets = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
            let current = presets
                .iter()
                .position(|p| *p == app.config.performance.nvenc_preset)
                .unwrap_or(3); // default to p4 index
            let next = if increase {
                (current + 1) % presets.len()
            } else {
                (current + presets.len() - 1) % presets.len()
            };
            app.config.performance.nvenc_preset = presets[next].to_string();
        }
        ConfigField::QualityPreset => cycle_quality_preset(app, increase),
        ConfigField::SameDirectory => {
            app.config.output.same_directory = !app.config.output.same_directory;
        }
        ConfigField::DaemonEnabled => {
            app.config.daemon.enabled = !app.config.daemon.enabled;
        }
        ConfigField::RfSd
        | ConfigField::RfHd
        | ConfigField::RfFullHd
        | ConfigField::RfFullHdHdr
        | ConfigField::RfFullHdDv
        | ConfigField::RfUhd
        | ConfigField::RfUhdHdr
        | ConfigField::RfUhdDv => {
            let encoder = app.config.encoder;
            if let Some(preset) = preset_for_rf_field(&mut app.config.presets, field) {
                adjust_preset_rf(preset, encoder, increase);
            }
        }
        // Text fields are edited via Enter, not ← →
        ConfigField::OutputSuffix
        | ConfigField::OutputContainer
        | ConfigField::AudioLanguages
        | ConfigField::SubtitleLanguages
        | ConfigField::DaemonBindAddress
        | ConfigField::DaemonPort => {}
    }
}

/// Cycle the overall quality preset and apply its per-tier values.
///
/// `Low`/`Medium`/`High` overwrite the per-tier presets; `Custom` keeps the
/// user's own values. Toggling visibility of the RF rows can shrink the list,
/// so the selection index is clamped afterwards.
fn cycle_quality_preset(app: &mut App, increase: bool) {
    let next = if increase {
        app.config.quality_preset.next()
    } else {
        app.config.quality_preset.prev()
    };
    app.config.quality_preset = next;
    if let Some(presets) = next.presets() {
        app.config.presets = presets;
    }
    let count = crate::ui::config_screen::visible_config_items(&app.config).len();
    if app.config_selected >= count {
        app.config_selected = count.saturating_sub(1);
    }
}

/// Map a per-resolution rate-factor field to its mutable preset, if any.
fn preset_for_rf_field(
    presets: &mut crate::config::EncodingPresetsConfig,
    field: crate::ui::config_screen::ConfigField,
) -> Option<&mut crate::config::EncodingPreset> {
    use crate::ui::config_screen::ConfigField;
    Some(match field {
        ConfigField::RfSd => &mut presets.sd,
        ConfigField::RfHd => &mut presets.hd,
        ConfigField::RfFullHd => &mut presets.full_hd,
        ConfigField::RfFullHdHdr => &mut presets.full_hd_hdr,
        ConfigField::RfFullHdDv => &mut presets.full_hd_dv,
        ConfigField::RfUhd => &mut presets.uhd,
        ConfigField::RfUhdHdr => &mut presets.uhd_hdr,
        ConfigField::RfUhdDv => &mut presets.uhd_dv,
        _ => return None,
    })
}

fn adjust_preset_rf(
    preset: &mut crate::config::EncodingPreset,
    encoder: crate::config::Encoder,
    increase: bool,
) {
    use crate::config::Encoder;
    let val = match encoder {
        Encoder::SvtAv1 => &mut preset.crf,
        Encoder::Nvenc => &mut preset.nvenc_cq,
        Encoder::Qsv => &mut preset.qsv_quality,
        Encoder::Amf => &mut preset.amf_quality,
    };
    if increase {
        *val = val.saturating_add(1).min(encoder.max_quality());
    } else {
        *val = val.saturating_sub(1);
    }
}
