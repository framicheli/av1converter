use crate::app::MessageKind;
use crate::i18n::{Language, Msg, t};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::ListItem,
};

/// Rows `text` fills when word-wrapped to `width` cells. Words break at
/// spaces; a word wider than the row breaks between characters, as ratatui
/// wraps it.
pub fn wrapped_rows(text: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = text
        .lines()
        .map(|line| {
            let mut rows = 1;
            let mut used = 0;
            for word in line.split(' ').filter(|word| !word.is_empty()) {
                let word_width = Line::raw(word).width();
                if used > 0 && used + 1 + word_width <= width {
                    used += 1 + word_width;
                    continue;
                }
                if used > 0 {
                    rows += 1;
                    used = 0;
                }
                if word_width <= width {
                    used = word_width;
                    continue;
                }
                let mut buf = [0u8; 4];
                for ch in word.chars() {
                    let ch_width = Line::raw(&*ch.encode_utf8(&mut buf)).width();
                    if used + ch_width > width {
                        rows += 1;
                        used = 0;
                    }
                    used += ch_width;
                }
            }
            rows
        })
        .sum();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Translate a job status `reason` string for display.
///
/// Reasons are stored in English internally (some are compared as sentinels,
/// e.g. `"Cancelled"`). Known reasons are localized here; anything else (such as
/// a raw error from an external tool) is passed through unchanged.
pub fn translate_reason(lang: Language, reason: &str) -> String {
    match reason {
        "Cancelled" => t(lang, Msg::Cancelled).to_string(),
        other => other.to_string(),
    }
}

/// Create a centered rectangle within a given area
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let percent_x = percent_x.min(100);
    let percent_y = percent_y.min(100);
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Get color for VMAF score/threshold
pub fn get_vmaf_color(score: f64) -> Color {
    if score >= 95.0 {
        Color::Cyan
    } else if score >= 90.0 {
        Color::Green
    } else if score >= 85.0 {
        Color::Yellow
    } else if score >= 80.0 {
        Color::Rgb(255, 165, 0)
    } else {
        Color::Red
    }
}

pub fn message_color(kind: MessageKind) -> Color {
    match kind {
        MessageKind::Info => Color::Cyan,
        MessageKind::Success => Color::Green,
        MessageKind::Warning => Color::Yellow,
        MessageKind::Error => Color::Red,
    }
}

/// Create a menu item with selection styling
pub fn create_menu_item(text: &str, index: usize, selected: usize) -> ListItem<'static> {
    let style = if index == selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let prefix = if index == selected { "> " } else { "  " };
    ListItem::new(format!("{prefix}{text}")).style(style)
}

pub use crate::i18n::quality_description as get_quality_description;

#[cfg(test)]
mod tests {
    use super::wrapped_rows;

    #[test]
    fn a_long_cjk_word_fills_rows_by_whole_characters() {
        assert_eq!(wrapped_rows(&"字".repeat(41), 41), 3);
        assert_eq!(wrapped_rows(&"字".repeat(20), 41), 1);
        assert_eq!(wrapped_rows(&"a".repeat(82), 41), 2);
        assert_eq!(wrapped_rows("word word", 4), 2);
    }
}
