use super::{AudioTrack, SubtitleTrack};
use crate::config::TrackPresetConfig;
use crate::tracks::TrackSelection;

/// Auto-select tracks based on language preferences from config
pub fn auto_select_tracks(
    audio_tracks: &[AudioTrack],
    subtitle_tracks: &[SubtitleTrack],
    config: &TrackPresetConfig,
) -> TrackSelection {
    let audio_indices = select_by_language(
        audio_tracks
            .iter()
            .map(|t| (t.index, t.language.as_deref())),
        &config.preferred_audio_languages,
        config.select_all_fallback,
        audio_tracks.len(),
    );

    let subtitle_indices = select_by_language(
        subtitle_tracks
            .iter()
            .map(|t| (t.index, t.language.as_deref())),
        &config.preferred_subtitle_languages,
        config.select_all_fallback,
        subtitle_tracks.len(),
    );

    TrackSelection {
        audio_indices,
        subtitle_indices,
    }
}

/// Select tracks matching preferred languages
fn select_by_language<'a>(
    tracks: impl Iterator<Item = (usize, Option<&'a str>)>,
    preferred: &[String],
    select_all_fallback: bool,
    total_count: usize,
) -> Vec<usize> {
    let tracks: Vec<(usize, Option<&str>)> = tracks.collect();

    if preferred.is_empty() || tracks.is_empty() {
        if select_all_fallback {
            return tracks.iter().map(|(i, _)| *i).collect();
        }
        return Vec::new();
    }

    let mut selected = Vec::new();
    for pref_lang in preferred {
        for &(index, lang) in &tracks {
            if let Some(l) = lang
                && l.eq_ignore_ascii_case(pref_lang)
                && !selected.contains(&index)
            {
                selected.push(index);
            }
        }
    }

    // If nothing matched, select all or none
    if selected.is_empty() && select_all_fallback {
        return (0..total_count).collect();
    }

    selected.sort();
    selected
}
