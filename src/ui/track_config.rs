use crate::app::{App, TrackFocus};
use crate::i18n::{Msg, t};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

#[allow(clippy::too_many_lines)]
pub fn render_track_config(f: &mut Frame, app: &mut App) {
    let lang = app.config.language;
    let (
        filename,
        resolution_string,
        hdr_string,
        audio_data,
        subtitle_data,
        remux_only,
        output_filename,
    ) = {
        let Some(job) = app.current_config_job() else {
            return;
        };

        let audio_data: Vec<(String, String, String, bool)> = job
            .audio_tracks
            .iter()
            .map(|track| {
                (
                    track.display_name(),
                    track.bitrate_string(),
                    track.sample_rate_string(),
                    job.track_selection.audio_indices.contains(&track.index),
                )
            })
            .collect();

        let subtitle_data: Vec<(String, bool, bool)> = job
            .subtitle_tracks
            .iter()
            .map(|track| {
                (
                    track.display_name(),
                    track.forced,
                    job.track_selection.subtitle_indices.contains(&track.index),
                )
            })
            .collect();

        let output_filename = job
            .output_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || t(lang, Msg::Unknown).to_string(),
                |f| f.to_string_lossy().into_owned(),
            );

        (
            job.filename(),
            job.resolution_string(),
            job.hdr_string().to_string(),
            audio_data,
            subtitle_data,
            job.remux_only,
            output_filename,
        )
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(f.area());

    // File info header
    let info_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::FileLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                filename,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::ResolutionLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(resolution_string, Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled(
                format!("{}: ", t(lang, Msg::TypeLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                match hdr_string.as_str() {
                    "Dolby Vision" => "Dolby Vision → HDR10".to_string(),
                    _ => hdr_string.clone(),
                },
                Style::default().fg(match hdr_string.as_str() {
                    "HDR10" => Color::Yellow,
                    "HLG" => Color::Green,
                    "Dolby Vision" => Color::Magenta,
                    _ => Color::White,
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}: ", t(lang, Msg::ModeLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                t(
                    lang,
                    if remux_only {
                        Msg::RemuxOnly
                    } else {
                        Msg::EncodeVideo
                    },
                ),
                Style::default()
                    .fg(if remux_only {
                        Color::Green
                    } else {
                        Color::Yellow
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{}: ", t(lang, Msg::OutputFileLabel)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(output_filename, Style::default().fg(Color::Cyan)),
        ]),
    ];

    let total_jobs = app.queue.jobs.len();
    let info_title = if total_jobs > 1 {
        format!(
            " {} ({}/{}) ",
            t(lang, Msg::VideoInfo),
            app.queue.config_job_index + 1,
            total_jobs
        )
    } else {
        format!(" {} ", t(lang, Msg::VideoInfo))
    };

    let info = Paragraph::new(info_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(info_title),
    );
    f.render_widget(info, chunks[0]);

    // Track selection area
    let track_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Audio tracks with bitrate/sample rate
    let audio_items: Vec<ListItem> = audio_data
        .iter()
        .enumerate()
        .map(|(i, (name, bitrate, sample_rate, selected))| {
            let is_cursor = app.track_focus == TrackFocus::Audio && i == app.audio_cursor;
            create_audio_track_item(name, bitrate, sample_rate, *selected, is_cursor)
        })
        .collect();

    let audio_border_color = if app.track_focus == TrackFocus::Audio {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let audio_list = List::new(audio_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(audio_border_color))
                .title(format!(
                    " {} [{}] ",
                    t(lang, Msg::AudioTracks),
                    t(lang, Msg::SpaceToToggle)
                )),
        )
        .highlight_style(Style::default());

    app.audio_list_state.select(Some(app.audio_cursor));
    f.render_stateful_widget(audio_list, track_chunks[0], &mut app.audio_list_state);

    // Subtitle tracks with forced flag
    let subtitle_items: Vec<ListItem> = subtitle_data
        .iter()
        .enumerate()
        .map(|(i, (name, forced, selected))| {
            let is_cursor = app.track_focus == TrackFocus::Subtitle && i == app.subtitle_cursor;
            create_subtitle_track_item(name, *forced, *selected, is_cursor, lang)
        })
        .collect();

    let subtitle_border_color = if app.track_focus == TrackFocus::Subtitle {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let subtitle_list = List::new(subtitle_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(subtitle_border_color))
                .title(format!(
                    " {} [{}] ",
                    t(lang, Msg::SubtitleTracks),
                    t(lang, Msg::SpaceToToggle)
                )),
        )
        .highlight_style(Style::default());

    app.subtitle_list_state.select(Some(app.subtitle_cursor));
    f.render_stateful_widget(subtitle_list, track_chunks[1], &mut app.subtitle_list_state);

    // Help / Confirm button
    let confirm_style = if app.track_focus == TrackFocus::Confirm {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let mut help_spans = vec![
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::SwitchPanel))),
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::Navigate))),
        Span::styled("Space", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::Toggle))),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::SwitchMode))),
        Span::styled("a", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::AllAudio))),
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(format!(" {}  ", t(lang, Msg::AllSubs))),
    ];
    if total_jobs > 1 {
        help_spans.push(Span::styled("←→", Style::default().fg(Color::Yellow)));
        help_spans.push(Span::raw(format!(" {}  ", t(lang, Msg::SwitchFile))));
    }
    help_spans.push(Span::styled(" [", Style::default().fg(Color::DarkGray)));
    help_spans.push(Span::styled(
        format!(" {} ", t(lang, Msg::Continue)),
        confirm_style,
    ));
    help_spans.push(Span::styled("]  ", Style::default().fg(Color::DarkGray)));
    help_spans.push(Span::styled("q", Style::default().fg(Color::Yellow)));
    help_spans.push(Span::raw(format!(" {}", t(lang, Msg::Quit))));

    let help_text = Line::from(help_spans);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[2]);
}

fn create_audio_track_item(
    name: &str,
    bitrate: &str,
    sample_rate: &str,
    selected: bool,
    is_cursor: bool,
) -> ListItem<'static> {
    let checkbox = if selected { "[x]" } else { "[ ]" };
    let prefix = if is_cursor { "> " } else { "  " };
    let extra = format!(" ({bitrate}, {sample_rate})");

    let style = if is_cursor {
        Style::default().add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    ListItem::new(format!("{prefix}{checkbox} {name}{extra}")).style(style)
}

fn create_subtitle_track_item(
    name: &str,
    forced: bool,
    selected: bool,
    is_cursor: bool,
    lang: crate::i18n::Language,
) -> ListItem<'static> {
    let checkbox = if selected { "[x]" } else { "[ ]" };
    let prefix = if is_cursor { "> " } else { "  " };
    let forced_str = if forced {
        format!(" [{}]", t(lang, Msg::ForcedTag))
    } else {
        String::new()
    };

    let style = if is_cursor {
        Style::default().add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    ListItem::new(format!("{prefix}{checkbox} {name}{forced_str}")).style(style)
}
