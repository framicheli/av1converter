mod analyzer;
mod app;
mod config;
mod daemon;
mod disc;
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
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, widgets::Clear};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::app::HOME_MENU;
use crate::i18n::{Msg, t};

const USAGE: &str = "\
Usage: av1converter [OPTION]

  (no option)          start the interactive TUI
  --daemon             run the web-UI daemon in the background (must be enabled in Settings)
  --daemon-foreground  run the daemon in the foreground, logging to stdout
  --stop               stop the background daemon
  --status             show whether the daemon is running
  --scan-discs         list optical drives and the titles on the loaded disc
  --purge              delete configuration and daemon state after confirmation
  --help               show this help
  --version            show the version
";

#[derive(Debug, PartialEq)]
enum Cli {
    Tui,
    Daemon,
    DaemonForeground,
    Stop,
    Status,
    ScanDiscs,
    Purge,
    Help,
    Version,
}

const FLAGS: &[&str] = &[
    "--daemon",
    "--daemon-foreground",
    "--stop",
    "--status",
    "--scan-discs",
    "--purge",
    "--help",
    "-h",
    "--version",
    "-V",
];

fn parse_flag(arg: &str) -> Option<Cli> {
    Some(match arg {
        "--daemon" => Cli::Daemon,
        "--daemon-foreground" => Cli::DaemonForeground,
        "--stop" => Cli::Stop,
        "--status" => Cli::Status,
        "--scan-discs" => Cli::ScanDiscs,
        "--purge" => Cli::Purge,
        "--help" | "-h" => Cli::Help,
        "--version" | "-V" => Cli::Version,
        _ => return None,
    })
}

fn parse_cli(args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Cli, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Cli::Tui);
    };
    let first = first.as_ref();
    let Some(cli) = parse_flag(first) else {
        return Err(unknown_arg_message(first));
    };
    if let Some(extra) = args.next() {
        return Err(format!(
            "Unexpected extra argument: {}\n{USAGE}",
            extra.as_ref()
        ));
    }
    Ok(cli)
}

fn unknown_arg_message(arg: &str) -> String {
    match suggest_flag(arg) {
        Some(flag) => format!("Unknown argument: {arg}\nDid you mean `{flag}`?\n{USAGE}"),
        None => format!("Unknown argument: {arg}\n{USAGE}"),
    }
}

fn suggest_flag(arg: &str) -> Option<&'static str> {
    let mut best: Option<(&'static str, usize)> = None;
    for flag in FLAGS {
        let d = edit_distance(arg, flag);
        if (1..=2).contains(&d) && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((flag, d));
        }
    }
    best.map(|(flag, _)| flag)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut curr = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            curr.push(if ca == cb {
                prev[j]
            } else {
                1 + prev[j].min(prev[j + 1]).min(curr[j])
            });
        }
        prev = curr;
    }
    prev[b.len()]
}

/// Headless daemon entry: refuses to start unless enabled in the config or if
/// an instance is already running. In background mode the process re-execs
/// itself detached and the parent only reports the outcome.
fn run_daemon_entry(foreground: bool) -> io::Result<()> {
    let mut config = config::AppConfig::load();
    let lang = config.language;
    if !config.daemon.enabled {
        eprintln!("{}", t(lang, Msg::DaemonDisabledError));
        std::process::exit(1);
    }
    if let Some(pid) = daemon::lifecycle::running_pid() {
        eprintln!("{} (PID {pid})", t(lang, Msg::DaemonAlreadyRunning));
        std::process::exit(1);
    }

    // The API can browse the filesystem, queue encodes and rewrite the
    // configuration, so a token is minted on first start. The printed URL
    // carries it, so one click authorises the browser.
    if config.daemon.auth_token.len() < 32 {
        config.daemon.auth_token =
            config::DaemonConfig::generate_token().map_err(io::Error::other)?;
        if let Err(e) = config.save() {
            eprintln!("{} ({e})", t(lang, Msg::SaveFailed));
            std::process::exit(1);
        }
        println!("{}", t(lang, Msg::DaemonTokenGenerated));
    }
    if config.daemon.binds_publicly() {
        eprintln!("{}", t(lang, Msg::DaemonPublicHttp));
    }

    if !foreground {
        match daemon::lifecycle::spawn_background() {
            Ok(pid) => {
                println!("{} (PID {pid})", t(lang, Msg::DaemonStarted));
                println!("{} {}", t(lang, Msg::DaemonListening), config.daemon.url());
                println!("{}", t(lang, Msg::DaemonStopHint));
                return Ok(());
            }
            Err(e) => {
                eprintln!(
                    "{} {} ({e})",
                    t(lang, Msg::DaemonStartFailed),
                    daemon::lifecycle::log_file().display()
                );
                std::process::exit(1);
            }
        }
    }

    utils::init_daemon_logging();
    let _pid_guard = daemon::lifecycle::write_pid_file()?;
    daemon::run_daemon(config).map_err(io::Error::other)
}

/// `--stop`: signal the background daemon and wait for it to exit.
fn stop_daemon_entry() {
    let lang = config::AppConfig::load().language;
    let Some(pid) = daemon::lifecycle::running_pid() else {
        println!("{}", t(lang, Msg::DaemonNotRunning));
        return;
    };
    match daemon::lifecycle::stop(pid) {
        Ok(()) => {
            println!("{} (PID {pid})", t(lang, Msg::DaemonStopped));
        }
        Err(e) => {
            eprintln!("{} {e}", t(lang, Msg::DaemonStopFailed));
            std::process::exit(1);
        }
    }
}

/// `--status`: report whether the background daemon is running.
fn daemon_status_entry() {
    let config = config::AppConfig::load();
    let lang = config.language;
    match daemon::lifecycle::running_pid() {
        Some(pid) => {
            println!("{} (PID {pid})", t(lang, Msg::DaemonRunning));
            println!("{} {}", t(lang, Msg::DaemonListening), config.daemon.url());
        }
        None => println!("{}", t(lang, Msg::DaemonNotRunning)),
    }
}

/// `--scan-discs`: print every drive and the titles of each loaded disc, as
/// parsed from `MakeMKV`'s output. English, like `--help`.
fn scan_discs_entry() {
    let config = config::AppConfig::load();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bin = match disc::find_makemkvcon(&config) {
        Ok(bin) => bin,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("makemkvcon: {}", bin.display());

    let drives = match disc::list_drives(&bin, &cancel) {
        Ok(drives) => drives,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    for drive in &drives {
        println!(
            "\ndrive {}: {} [{}]",
            drive.id,
            drive.name,
            drive.disc_label.as_deref().unwrap_or("empty")
        );
        if drive.disc_label.is_none() {
            continue;
        }
        match disc::scan_titles(&bin, drive.id, &cancel) {
            Ok(scan) => {
                if let Some(kind) = scan.disc_type {
                    println!("  {kind}");
                }
                for title in scan.titles {
                    println!(
                        "  title {:>2}  {}  {}  {} chapters  {}",
                        title.id,
                        utils::format_duration(title.duration),
                        utils::format_file_size(title.size_bytes),
                        title.chapters,
                        title.name
                    );
                    for track in &title.tracks {
                        println!("            {track}");
                    }
                }
            }
            Err(e) => eprintln!("  {e}"),
        }
    }
}

/// Directories `--purge` removes: config and daemon data. Deduped in case both
/// resolve to the same path (no `HOME`/`XDG_*`, so both fall back to `.`).
fn purge_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = config::AppConfig::config_path().parent() {
        dirs.push(dir.to_path_buf());
    }
    let data = daemon::lifecycle::data_dir();
    if !dirs.contains(&data) {
        dirs.push(data);
    }
    dirs
}

fn is_purge_yes(line: &str) -> bool {
    let line = line.trim();
    line.eq_ignore_ascii_case("y") || line.eq_ignore_ascii_case("yes")
}

/// `--purge`: delete configuration and daemon state after confirmation.
/// English, like `--help`. Does not load the config, which would create one.
fn purge_entry() {
    if let Some(pid) = daemon::lifecycle::running_pid() {
        eprintln!(
            "The daemon is still running (PID {pid}). Stop it first with: av1converter --stop"
        );
        std::process::exit(1);
    }

    let existing: Vec<PathBuf> = purge_dirs()
        .into_iter()
        .filter(|dir| dir.exists())
        .collect();
    if existing.is_empty() {
        println!("Nothing to delete.");
        return;
    }

    println!("This will permanently delete:");
    for dir in &existing {
        println!("  {}", dir.display());
    }
    print!("Are you sure? [y/N] ");
    let _ = io::stdout().flush();

    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(_) if is_purge_yes(&line) => {}
        Ok(_) => {
            println!("Cancelled.");
            return;
        }
        Err(e) => {
            eprintln!("Could not read confirmation: {e}");
            std::process::exit(1);
        }
    }

    for dir in &existing {
        if let Err(e) = std::fs::remove_dir_all(dir) {
            eprintln!("Could not delete {}: {e}", dir.display());
            std::process::exit(1);
        }
        println!("Deleted {}", dir.display());
    }
}

fn main() -> io::Result<()> {
    match parse_cli(std::env::args().skip(1)) {
        Ok(Cli::Tui) => {}
        Ok(Cli::Daemon) => return run_daemon_entry(false),
        Ok(Cli::DaemonForeground) => return run_daemon_entry(true),
        Ok(Cli::Stop) => {
            stop_daemon_entry();
            return Ok(());
        }
        Ok(Cli::Status) => {
            daemon_status_entry();
            return Ok(());
        }
        Ok(Cli::ScanDiscs) => {
            scan_discs_entry();
            return Ok(());
        }
        Ok(Cli::Purge) => {
            purge_entry();
            return Ok(());
        }
        Ok(Cli::Help) => {
            print!("{USAGE}");
            return Ok(());
        }
        Ok(Cli::Version) => {
            println!("av1converter {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Err(msg) => {
            eprint!("{msg}");
            std::process::exit(2);
        }
    }

    let _log_guard = utils::init_logging();

    // Restore the terminal even if panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, EnterAlternateScreen) {
        let _ = restore_terminal();
        return Err(e);
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(e) => {
            let _ = restore_terminal();
            return Err(e);
        }
    };

    // Create app and run
    let mut app = App::new();
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    let restore = restore_terminal();
    let cursor = terminal.show_cursor();
    res?;
    restore?;
    cursor
}

/// Best-effort terminal restoration used by setup errors, runtime errors and
/// the panic hook. Both operations are attempted even when the first fails.
fn restore_terminal() -> io::Result<()> {
    let raw = disable_raw_mode();
    let screen = execute!(io::stdout(), LeaveAlternateScreen);
    raw.and(screen)
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        app.process_progress_messages();
        app.process_analysis_messages();
        app.process_disc_events();
        app.tick_message();

        terminal.draw(|f| {
            f.render_widget(Clear, f.area());
            if ui::terminal_too_small(f.area()) {
                ui::render_too_small(f, app.config.language);
            } else {
                match app.current_screen {
                    Screen::Home => ui::render_home(f, app),
                    Screen::FileExplorer { .. } => ui::render_explorer(f, app),
                    Screen::FileConfirm => ui::render_file_confirm(f, app),
                    Screen::DiscDrives => ui::render_disc_drives(f, app),
                    Screen::DiscTitles => ui::render_disc_titles(f, app),
                    Screen::TrackConfig => ui::render_track_config(f, app),
                    Screen::Queue => ui::render_queue(f, app),
                    Screen::Finish => ui::render_finish(f, app),
                    Screen::Configuration => ui::render_config_screen(f, app),
                }
                if app.dv_dialog.is_some() && app.current_screen == Screen::TrackConfig {
                    ui::render_dv_dialog(f, app);
                }
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

        // The rip is waited out too: quitting while makemkvcon still runs
        // would leave the child behind.
        if app.should_quit
            && !app.encoding_active
            && app.analysis_receiver.is_none()
            && app.disc_receiver.is_none()
        {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, key: KeyCode) {
    if app.should_quit {
        return;
    }
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
        Screen::DiscDrives | Screen::DiscTitles => handle_disc_key(app, key),
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
            app.cancel_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
            app.analysis_cancel_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
            // An in-flight rip is killed here rather than outliving the TUI as
            // an orphan makemkvcon.
            app.disc_cancel_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
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
            3 => app.start_disc_flow(),                  // Rip DVD / Blu-ray
            4 => app.navigate_to_configuration(),        // Configuration
            5 => {
                app.confirm_dialog = Some((ConfirmAction::ExitApp, false));
            }
            _ => {}
        },
        _ => {}
    }
}

/// Drive and title selection. Esc during a scan cancels it and steps back.
fn handle_disc_key(app: &mut App, key: KeyCode) {
    app.clear_message();

    match key {
        KeyCode::Up | KeyCode::Char('k') => app.disc_move_up(),
        KeyCode::Down | KeyCode::Char('j') => app.disc_move_down(),
        KeyCode::Esc => app.leave_disc_screen(),
        KeyCode::Char(' ') if app.current_screen == Screen::DiscTitles => app.toggle_disc_title(),
        KeyCode::Enter => match app.current_screen {
            Screen::DiscDrives => app.scan_disc(app.disc_drive_cursor),
            _ => app.start_disc_rip(),
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

#[allow(clippy::too_many_lines)]
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
                    job.track_selection.audio_to_opus.clear();
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
        KeyCode::Char('o') => {
            let cursor = app.audio_cursor;
            if let Some(job) = app.current_config_job_mut()
                && let Some(track) = job.audio_tracks.get(cursor)
            {
                let idx = track.index;
                job.track_selection.toggle_audio_opus(idx);
            }
        }
        KeyCode::Char('O') => {
            if let Some(job) = app.current_config_job_mut() {
                // All-or-nothing across the selected tracks: a second press
                // undoes the first.
                let selected = job.track_selection.audio_indices.clone();
                let all_opus = !selected.is_empty()
                    && selected.iter().all(|&i| job.track_selection.is_opus(i));
                for idx in selected {
                    job.track_selection.set_audio_opus(idx, !all_opus);
                }
            }
        }
        KeyCode::Char('r' | 'R') => {
            let output_config = app.config.output.clone();
            if let Some(job) = app.current_config_job_mut() {
                job.remux_only = !job.remux_only;
                job.generate_output_path(&output_config);
            }
            crate::queue::make_output_paths_unique(&mut app.queue.jobs);
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
        // A title that finished ripping while an encode ran is waiting for its
        // tracks; Enter opens it.
        KeyCode::Enter if app.has_jobs_awaiting_config() => app.configure_next_job(),
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

    let config_item_count = ui::config_screen::visible_config_items(&app.config).len();

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
            app.config.sanitize();
            if !app.config.output.same_directory
                && !app
                    .config
                    .output
                    .output_directory
                    .as_deref()
                    .is_some_and(|path| std::path::Path::new(path).is_dir())
            {
                app.set_timed_message(
                    &format!(
                        "{}: {}",
                        t(lang, Msg::SaveFailed),
                        t(lang, Msg::WebCfgOutputDirectory)
                    ),
                    3,
                );
                return;
            }
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
        ConfigField::OutputDirectory => app
            .config
            .output
            .output_directory
            .clone()
            .unwrap_or_default(),
        ConfigField::AudioLanguages => app.config.tracks.preferred_audio_languages.join(", "),
        ConfigField::SubtitleLanguages => app.config.tracks.preferred_subtitle_languages.join(", "),
        ConfigField::DaemonBindAddress => app.config.daemon.bind_address.clone(),
        ConfigField::DaemonPort => app.config.daemon.port.to_string(),
        ConfigField::DaemonBrowseRoot => app.config.daemon.browse_root.clone(),
        ConfigField::DaemonAuthToken => app.config.daemon.auth_token.clone(),
        ConfigField::DiscStagingDirectory => app
            .config
            .disc
            .staging_directory
            .clone()
            .unwrap_or_default(),
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
        ConfigField::OutputDirectory => {
            app.config.output.output_directory = (!value.is_empty()).then_some(value);
        }
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
        // Both accept an empty value, which turns the feature off
        ConfigField::DaemonBrowseRoot => app.config.daemon.browse_root = value,
        ConfigField::DaemonAuthToken => app.config.daemon.auth_token = value,
        // Empty means the system temp directory.
        ConfigField::DiscStagingDirectory => {
            app.config.disc.staging_directory = (!value.is_empty()).then_some(value);
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

#[allow(clippy::too_many_lines)]
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
            let presets = crate::config::PerformanceConfig::NVENC_PRESETS;
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
        ConfigField::AudioDefaultMode => {
            app.config.audio.default_mode = if increase {
                app.config.audio.default_mode.next()
            } else {
                app.config.audio.default_mode.prev()
            };
        }
        ConfigField::OpusBitratePerChannel => {
            use crate::config::AudioConfig;
            let current = app.config.audio.opus_bitrate_per_channel;
            let next = if increase {
                current.saturating_add(8)
            } else {
                current.saturating_sub(8)
            };
            app.config.audio.opus_bitrate_per_channel =
                next.clamp(AudioConfig::MIN_PER_CHANNEL, AudioConfig::MAX_PER_CHANNEL);
        }
        ConfigField::SkipAlreadyOpus => {
            app.config.audio.skip_already_opus = !app.config.audio.skip_already_opus;
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
        | ConfigField::OutputDirectory
        | ConfigField::AudioLanguages
        | ConfigField::SubtitleLanguages
        | ConfigField::DaemonBindAddress
        | ConfigField::DaemonPort
        | ConfigField::DaemonBrowseRoot
        | ConfigField::DaemonAuthToken
        | ConfigField::DiscStagingDirectory => {}
    }
}

/// Cycle the overall quality preset and apply its per-tier values.
///
/// `Low`/`Medium`/`High` overwrite the per-tier presets; `Custom` keeps the
/// user's own values. Toggling visibility of the RF rows can shrink the list,
/// so the selection index is clamped afterward.
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
    let count = ui::config_screen::visible_config_items(&app.config).len();
    if app.config_selected >= count {
        app.config_selected = count.saturating_sub(1);
    }
}

/// Map a per-resolution rate-factor field to its mutable preset, if any.
fn preset_for_rf_field(
    presets: &mut config::EncodingPresetsConfig,
    field: ui::config_screen::ConfigField,
) -> Option<&mut config::EncodingPreset> {
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

fn adjust_preset_rf(preset: &mut config::EncodingPreset, encoder: config::Encoder, increase: bool) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_starts_the_tui() {
        assert_eq!(parse_cli([] as [&str; 0]).unwrap(), Cli::Tui);
    }

    #[test]
    fn a_valid_flag_is_accepted() {
        assert_eq!(parse_cli(["--stop"]).unwrap(), Cli::Stop);
        assert_eq!(parse_cli(["--purge"]).unwrap(), Cli::Purge);
    }

    #[test]
    fn purge_confirmation_accepts_only_yes() {
        assert!(is_purge_yes("y"));
        assert!(is_purge_yes("Yes\n"));
        assert!(!is_purge_yes("n"));
        assert!(!is_purge_yes("\n"));
        assert!(!is_purge_yes("yeah"));
    }

    #[test]
    fn purge_dirs_are_unique() {
        let dirs = purge_dirs();
        assert!(!dirs.is_empty());
        for (i, dir) in dirs.iter().enumerate() {
            assert!(
                !dirs[i + 1..].iter().any(|other| other == dir),
                "duplicate purge path {}",
                dir.display()
            );
        }
    }

    #[test]
    fn a_typo_suggests_the_closest_flag() {
        let err = parse_cli(["--stiop"]).unwrap_err();
        assert!(err.contains("Did you mean `--stop`"), "{err}");
    }

    #[test]
    fn extra_arguments_are_rejected() {
        let err = parse_cli(["--stop", "foo"]).unwrap_err();
        assert!(err.contains("Unexpected extra argument: foo"), "{err}");
    }

    #[test]
    fn a_distant_unknown_flag_is_not_suggested() {
        let err = parse_cli(["--foo"]).unwrap_err();
        assert!(err.contains("Unknown argument: --foo"), "{err}");
        assert!(!err.contains("Did you mean"), "{err}");
    }
}
