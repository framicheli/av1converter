use crate::analyzer::{DvMode, HdrType};
use crate::app::{App, TrackFocus};
use crate::i18n::{Msg, t};
use crate::ui::common::message_color;
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

        // Resolved to the bitrate the encoder is asked for, including
        // already-Opus tracks, which are left alone.
        let output = job.output_path.clone().unwrap_or_default();
        let plan = job
            .track_selection
            .resolve_for(&job.audio_tracks, &audio_config, &output);

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

        let subtitle_data: Vec<SubtitleRow> = job
            .subtitle_tracks
            .iter()
            .map(|track| SubtitleRow {
                name: track.display_name(),
                forced: track.forced,
                selected: job.track_selection.subtitle_indices.contains(&track.index),
                dropped: crate::tracks::subtitle_codecs_for(&output, std::slice::from_ref(track))
                    .first()
                    .is_some_and(Option::is_none),
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
                    Some(DvMode::ToHdr10) => {
                        format!("Dolby Vision{profile} ({})", t(lang, Msg::DvConvertedTag))
                    }
                    None => format!("Dolby Vision{profile}"),
                }
            } else {
                job.metadata.as_ref().map_or_else(
                    || t(lang, Msg::Unknown).to_string(),
                    |_| job.hdr_string().to_string(),
                )
            }
        };

        (
            job.filename(),
            job.metadata.as_ref().map_or_else(
                || t(lang, Msg::Unknown).to_string(),
                |_| job.resolution_string(),
            ),
            job.hdr_string().to_string(),
            hdr_display,
            audio_data,
            subtitle_data,
            job.remux_only,
            output_filename,
        )
    };

    let notice = if app.is_track_configurable(app.queue.config_job_index) {
        app.message
            .clone()
            .map(|msg| (msg, message_color(app.message_kind)))
    } else {
        Some((t(lang, Msg::WebTracksLocked).to_string(), Color::Yellow))
    };
    let notice_rows = notice.as_ref().map_or(0, |(msg, _)| {
        crate::ui::common::wrapped_rows(msg, f.area().width.saturating_sub(4))
            .saturating_add(2)
            .min(6)
    });
    // The notice gives up rows to the track lists, which need a border pair
    // plus one entry; stacked lists need that twice.
    let reserved = 6 + if narrow { 5 + 6 } else { 3 + 3 };
    let room = f.area().height.saturating_sub(2).saturating_sub(reserved);
    let notice_rows = notice_rows.min(room.max(3));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(notice_rows),
            Constraint::Length(6),
            Constraint::Min(5),
            Constraint::Length(if narrow { 5 } else { 3 }),
        ])
        .margin(1)
        .split(f.area());

    if let Some((msg, color)) = notice {
        let notice = Paragraph::new(msg)
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(color)),
            );
        f.render_widget(notice, chunks[0]);
    }

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
            Span::styled(resolution_string, Style::default()),
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
                    _ => Color::Reset,
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

    let configurable = app
        .queue
        .jobs
        .iter()
        .enumerate()
        .filter(|(index, _)| app.is_track_configurable(*index))
        .count()
        .max(1);
    let current_number = app
        .queue
        .jobs
        .iter()
        .take(app.queue.config_job_index + 1)
        .enumerate()
        .filter(|(index, _)| app.is_track_configurable(*index))
        .count()
        .max(1);
    let info_title = if configurable > 1 {
        format!(
            " {} ({current_number}/{configurable}) ",
            t(lang, Msg::VideoInfo),
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
    f.render_widget(info, chunks[1]);

    // Track selection area
    let track_chunks = Layout::default()
        .direction(if narrow {
            Direction::Vertical
        } else {
            Direction::Horizontal
        })
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

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
        .map(|(i, row)| {
            let is_cursor = app.track_focus == TrackFocus::Subtitle && i == app.subtitle_cursor;
            create_subtitle_track_item(row, is_cursor, lang)
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
    if hdr_string == "Dolby Vision" && !remux_only {
        help_spans.push(Span::styled("d", Style::default().fg(Color::Yellow)));
        help_spans.push(Span::raw(format!("\u{a0}{}  ", t(lang, Msg::DvModeHelp))));
    }
    if configurable > 1 {
        help_spans.push(Span::styled("←→", Style::default().fg(Color::Yellow)));
        help_spans.push(Span::raw(format!("\u{a0}{}  ", t(lang, Msg::SwitchFile))));
    }
    let others_awaiting = app.queue.jobs.iter().enumerate().any(|(index, job)| {
        index != app.queue.config_job_index
            && matches!(job.status, crate::queue::JobStatus::AwaitingConfig)
    });
    if others_awaiting {
        help_spans.push(Span::styled("A", Style::default().fg(Color::Yellow)));
        help_spans.push(Span::raw(format!(
            "\u{a0}{}  ",
            t(lang, Msg::WebApplyRemaining)
        )));
    }
    help_spans.push(Span::styled(" [", Style::default().fg(Color::DarkGray)));
    help_spans.push(Span::styled(
        format!(" {} ", t(lang, Msg::Continue)),
        confirm_style,
    ));
    help_spans.push(Span::styled("]  ", Style::default().fg(Color::DarkGray)));
    help_spans.push(Span::styled("Esc", Style::default().fg(Color::Yellow)));
    help_spans.push(Span::raw(format!("\u{a0}{}  ", t(lang, Msg::Back))));
    help_spans.push(Span::styled("q", Style::default().fg(Color::Yellow)));
    help_spans.push(Span::raw(format!("\u{a0}{}", t(lang, Msg::Quit))));

    let help_text = Line::from(help_spans);

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[3]);
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

    ListItem::new(format!("{prefix}{checkbox} {}{target}{extra}", row.name)).style(style)
}

/// One row of the subtitle panel, already resolved against the output container.
struct SubtitleRow {
    name: String,
    forced: bool,
    selected: bool,
    /// The output container cannot hold this track.
    dropped: bool,
}

fn create_subtitle_track_item(
    row: &SubtitleRow,
    is_cursor: bool,
    lang: crate::i18n::Language,
) -> ListItem<'static> {
    let checkbox = if row.selected { "[x]" } else { "[ ]" };
    let prefix = if is_cursor { "> " } else { "  " };
    let forced_str = if row.forced {
        format!(" [{}]", t(lang, Msg::ForcedTag))
    } else {
        String::new()
    };
    let dropped_str = if row.selected && row.dropped {
        format!(" ({})", t(lang, Msg::SubtitleNotIncluded))
    } else {
        String::new()
    };

    let style = if is_cursor {
        Style::default().add_modifier(Modifier::BOLD)
    } else if row.selected && row.dropped {
        Style::default().fg(Color::Yellow)
    } else if row.selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    ListItem::new(format!(
        "{prefix}{checkbox} {}{forced_str}{dropped_str}",
        row.name
    ))
    .style(style)
}
