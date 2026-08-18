use crate::analyzer::{DvMode, HdrType};
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
    let narrow = f.area().width < 100;
    let audio_config = app.config.audio.clone();
    let (
        filename,
        resolution_string,
        hdr_string,
        hdr_display,
        audio_data,
        subtitle_data,
        remux_only,
        output_filename,
    ) = {
        let Some(job) = app.current_config_job() else {
            return;
        };

        // Resolved so the row shows the bitrate the encoder is actually asked
        // for, including already-Opus tracks, which are left alone.
        let plan = job
            .track_selection
            .resolve(&job.audio_tracks, &audio_config);

        let audio_data: Vec<AudioRow> = job
            .audio_tracks
            .iter()
            .map(|track| AudioRow {
                name: track.display_name(),
                bitrate: track.bitrate_string(),
                sample_rate: track.sample_rate_string(),
                selected: job.track_selection.audio_indices.contains(&track.index),
                marked_opus: job.track_selection.is_opus(track.index),
                opus_kbps: plan
                    .audio
                    .iter()
                    .find(|p| p.source_index == track.index)
                    .and_then(|p| p.opus_kbps),
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

        // Dolby Vision sources show profile and the chosen conversion mode
        let hdr_display = {
            let meta = job.metadata.as_ref();
            if meta.is_some_and(|m| m.hdr_type == HdrType::DolbyVision) {
                let profile = meta
                    .and_then(|m| m.dv_profile)
                    .map_or_else(String::new, |p| format!(" P{p}"));
                match job.dv_mode {
                    Some(DvMode::KeepDolbyVision) => {
                        format!("Dolby Vision{profile} ({})", t(lang, Msg::DvKeptTag))
                    }
                    Some(DvMode::ToHdr10) => format!("Dolby Vision{profile} → HDR10"),
                    None => format!("Dolby Vision{profile}"),
                }
            } else {
                job.hdr_string().to_string()
            }
        };

        (
            job.filename(),
            job.resolution_string(),
            job.hdr_string().to_string(),
            hdr_display,
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
            Constraint::Length(if narrow { 5 } else { 3 }),
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
                hdr_display,
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
        .direction(if narrow {
            Direction::Vertical
        } else {
            Direction::Horizontal
        })
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Audio tracks with bitrate/sample rate
    let audio_items: Vec<ListItem> = audio_data
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_cursor = app.track_focus == TrackFocus::Audio && i == app.audio_cursor;
            create_audio_track_item(row, is_cursor, lang)
        })
        .collect();

    let audio_border_color = if app.track_focus == TrackFocus::Audio {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    // Opus is only reachable if FFmpeg was built with libopus; saying so on the
    // panel beats letting the encode fail an hour later.
    let opus_missing = !app.opus_deps && audio_data.iter().any(|r| r.opus_kbps.is_some());
    let audio_title = if opus_missing {
        format!(
            " {} [⚠ {}] ",
            t(lang, Msg::AudioTracks),
            t(lang, Msg::OpusUnavailable)
        )
    } else {
        format!(
            " {} [{}] ",
            t(lang, Msg::AudioTracks),
            t(lang, Msg::SpaceToToggle)
        )
    };

    let audio_list = List::new(audio_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if opus_missing {
                    Color::Yellow
                } else {
                    audio_border_color
                }))
                .title(audio_title),
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
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::SwitchPanel))),
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Navigate))),
        Span::styled("Space", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Toggle))),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::SwitchMode))),
        Span::styled("a", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::AllAudio))),
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::AllSubs))),
        Span::styled("o", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::ToOpus))),
        Span::styled("O", Style::default().fg(Color::Yellow)),
        Span::raw(format!("\u{a0}{}  ", t(lang, Msg::AllOpus))),
    ];
    if hdr_string == "Dolby Vision" {
        help_spans.push(Span::styled("d", Style::default().fg(Color::Yellow)));
        help_spans.push(Span::raw(format!("\u{a0}{}  ", t(lang, Msg::DvModeHelp))));
    }
    if total_jobs > 1 {
        help_spans.push(Span::styled("←→", Style::default().fg(Color::Yellow)));
        help_spans.push(Span::raw(format!("\u{a0}{}  ", t(lang, Msg::SwitchFile))));
    }
    help_spans.push(Span::styled(" [", Style::default().fg(Color::DarkGray)));
    help_spans.push(Span::styled(
        format!(" {} ", t(lang, Msg::Continue)),
        confirm_style,
    ));
    help_spans.push(Span::styled("]  ", Style::default().fg(Color::DarkGray)));
    help_spans.push(Span::styled("q", Style::default().fg(Color::Yellow)));
    help_spans.push(Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))));

    let help_text = Line::from(help_spans);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[2]);
}

/// One row of the audio panel, already resolved against the audio settings.
struct AudioRow {
    name: String,
    bitrate: String,
    sample_rate: String,
    selected: bool,
    marked_opus: bool,
    opus_kbps: Option<u32>,
}

fn create_audio_track_item(
    row: &AudioRow,
    is_cursor: bool,
    lang: crate::i18n::Language,
) -> ListItem<'static> {
    let checkbox = match (row.selected, row.opus_kbps) {
        (false, _) => "[ ]",
        (true, None) => "[x]",
        (true, Some(_)) => "[~]",
    };
    let prefix = if is_cursor { "> " } else { "  " };
    let extra = format!(" ({}, {})", row.bitrate, row.sample_rate);
    let target = match row.opus_kbps {
        Some(kbps) => format!(" → OPUS {kbps}k"),
        None if row.marked_opus && row.selected => {
            format!(" ({})", t(lang, Msg::AlreadyOpus))
        }
        None => String::new(),
    };

    let style = if is_cursor {
        Style::default().add_modifier(Modifier::BOLD)
    } else if row.opus_kbps.is_some() {
        Style::default().fg(Color::Cyan)
    } else if row.selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    ListItem::new(format!("{prefix}{checkbox} {}{extra}{target}", row.name)).style(style)
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
