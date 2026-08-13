//! Parsing of `makemkvcon -r` (robot mode) output.
//!
//! Every record is one line: a prefix, a colon, then quoted-CSV fields that
//! carry commas and quotes of their own — `DRV:0,2,999,12,"BD-RE","Movie,
//! The","/dev/sr0"`. Everything here parses through [`split_fields`].

use super::{DiscDrive, DiscTitle};
use std::collections::BTreeMap;
use std::time::Duration;

/// `MakeMKV` attribute ids, as used by `TINFO`/`SINFO`.
mod attr {
    pub const TYPE: u32 = 1;
    pub const NAME: u32 = 2;
    pub const LANG_NAME: u32 = 4;
    pub const CODEC_SHORT: u32 = 6;
    pub const CHAPTER_COUNT: u32 = 8;
    pub const DURATION: u32 = 9;
    pub const SIZE_BYTES: u32 = 11;
    pub const VIDEO_SIZE: u32 = 19;
    pub const OUTPUT_FILE_NAME: u32 = 27;
    pub const CHANNEL_LAYOUT: u32 = 40;
}

/// Split one robot-mode line into its prefix and its fields.
///
/// A line with no colon is rejected. Truncated output yields the fields that
/// arrived; callers check the count they need.
pub fn split_fields(line: &str) -> Option<(&str, Vec<String>)> {
    let (prefix, rest) = line.split_once(':')?;
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // A doubled quote inside a quoted field is a literal quote.
            // Backslashes stay literal: MakeMKV does not escape them.
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    fields.push(field);
    Some((prefix, fields))
}

/// `DRV:id,state,flags,drive flags,"drive name","disc name","device path"`.
///
/// `MakeMKV` lists every slot it knows about, most of them empty; a slot with
/// no drive name is skipped.
pub fn parse_drive(fields: &[String]) -> Option<DiscDrive> {
    let id = num(fields, 0)?;
    let name = fields.get(4)?.trim();
    if name.is_empty() {
        return None;
    }
    Some(DiscDrive {
        id,
        name: name.to_string(),
        disc_label: fields
            .get(5)
            .map(|label| label.trim())
            .filter(|label| !label.is_empty())
            .map(str::to_string),
    })
}

/// Accumulates the `CINFO`/`TINFO`/`SINFO` lines of one `info disc:N` run.
#[derive(Default)]
pub struct TitleScan {
    disc: BTreeMap<u32, String>,
    titles: BTreeMap<u32, BTreeMap<u32, String>>,
    streams: BTreeMap<(u32, u32), BTreeMap<u32, String>>,
}

impl TitleScan {
    pub fn feed(&mut self, prefix: &str, fields: &[String]) {
        match prefix {
            "CINFO" if fields.len() >= 3 => {
                if let Some(id) = num(fields, 0) {
                    self.disc.insert(id, fields[2].clone());
                }
            }
            "TINFO" if fields.len() >= 4 => {
                if let (Some(title), Some(id)) = (num(fields, 0), num(fields, 1)) {
                    self.titles
                        .entry(title)
                        .or_default()
                        .insert(id, fields[3].clone());
                }
            }
            "SINFO" if fields.len() >= 5 => {
                if let (Some(title), Some(stream), Some(id)) =
                    (num(fields, 0), num(fields, 1), num(fields, 2))
                {
                    self.streams
                        .entry((title, stream))
                        .or_default()
                        .insert(id, fields[4].clone());
                }
            }
            _ => {}
        }
    }

    /// What `MakeMKV` calls this disc — "Blu-ray disc", "DVD disc".
    pub fn disc_type(&self) -> Option<String> {
        self.disc
            .get(&attr::TYPE)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    pub fn finish(self) -> Vec<DiscTitle> {
        let mut tracks: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for ((title, _stream), attrs) in &self.streams {
            let summary = stream_summary(attrs);
            if !summary.is_empty() {
                tracks.entry(*title).or_default().push(summary);
            }
        }
        self.titles
            .into_iter()
            .map(|(id, attrs)| DiscTitle {
                id,
                name: [attr::NAME, attr::OUTPUT_FILE_NAME]
                    .iter()
                    .filter_map(|key| attrs.get(key))
                    .map(|value| value.trim())
                    .find(|value| !value.is_empty())
                    .map_or_else(|| format!("Title {id}"), str::to_string),
                duration: attrs
                    .get(&attr::DURATION)
                    .map_or(Duration::ZERO, |value| parse_duration(value)),
                size_bytes: attrs
                    .get(&attr::SIZE_BYTES)
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(0),
                chapters: attrs
                    .get(&attr::CHAPTER_COUNT)
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(0),
                tracks: tracks.remove(&id).unwrap_or_default(),
            })
            .collect()
    }
}

/// One line of "Video MPEG-2 720x480" / "Audio DTS 5.1 English" for the title
/// screen. The real track picker runs later, on the ripped file.
fn stream_summary(attrs: &BTreeMap<u32, String>) -> String {
    [
        attr::TYPE,
        attr::CODEC_SHORT,
        attr::VIDEO_SIZE,
        attr::CHANNEL_LAYOUT,
        attr::LANG_NAME,
    ]
    .iter()
    .filter_map(|key| attrs.get(key))
    .map(|value| value.trim())
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// The file name `MakeMKV` reports for `title` while ripping it, if this is that
/// line. The name is taken as a bare file name by the caller, never as a path.
pub fn output_file_name(fields: &[String], title: u32) -> Option<String> {
    if num(fields, 0)? != title || num(fields, 1)? != attr::OUTPUT_FILE_NAME {
        return None;
    }
    fields
        .get(3)
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.trim().to_string())
}

/// Overall completion from `PRGV:current,total,max`, as a percentage.
#[allow(clippy::cast_precision_loss)] // progress counters are well under 2^53
pub fn progress_percent(fields: &[String]) -> Option<f64> {
    let total: u64 = num64(fields, 1)?;
    let max: u64 = num64(fields, 2)?;
    if max == 0 {
        return None;
    }
    Some((total as f64 / max as f64 * 100.0).clamp(0.0, 100.0))
}

/// `"1:23:45"` or `"23:45"`. Anything unparseable is zero.
fn parse_duration(value: &str) -> Duration {
    let mut secs: u64 = 0;
    for part in value.trim().split(':') {
        let Ok(n) = part.trim().parse::<u64>() else {
            return Duration::ZERO;
        };
        secs = secs.saturating_mul(60).saturating_add(n);
    }
    Duration::from_secs(secs)
}

fn num(fields: &[String], index: usize) -> Option<u32> {
    fields.get(index)?.trim().parse().ok()
}

fn num64(fields: &[String], index: usize) -> Option<u64> {
    fields.get(index)?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(line: &str) -> Vec<String> {
        split_fields(line).expect("prefixed line").1
    }

    #[test]
    fn quoted_commas_stay_inside_their_field() {
        let (prefix, f) =
            split_fields(r#"DRV:0,2,999,12,"BD-RE","Movie, The","/dev/sr0""#).unwrap();
        assert_eq!(prefix, "DRV");
        assert_eq!(f.len(), 7);
        assert_eq!(f[4], "BD-RE");
        assert_eq!(f[5], "Movie, The");
        assert_eq!(f[6], "/dev/sr0");
    }

    #[test]
    fn doubled_quotes_are_one_literal_quote() {
        let f = fields(r#"TINFO:0,2,0,"The ""Director's Cut""",tail"#);
        assert_eq!(f[3], r#"The "Director's Cut""#);
        assert_eq!(f[4], "tail");
    }

    #[test]
    fn empty_fields_are_kept_in_place() {
        let f = fields(r#"DRV:1,256,999,0,"","","""#);
        assert_eq!(f.len(), 7);
        assert!(f[4].is_empty() && f[5].is_empty() && f[6].is_empty());
    }

    #[test]
    fn a_truncated_line_yields_what_arrived() {
        let f = fields(r#"TINFO:3,9,0,"1:2"#);
        assert_eq!(f, vec!["3", "9", "0", "1:2"]);
        assert!(split_fields("no colon here").is_none());
    }

    /// Output with nothing in it parses to nothing, rather than to a title.
    #[test]
    fn empty_output_yields_no_titles() {
        let mut scan = TitleScan::default();
        for line in ["", " ", ":", "TINFO:", "MSG:1005,0,1,\"started\",\"x\""] {
            if let Some((prefix, f)) = split_fields(line) {
                scan.feed(prefix, &f);
            }
        }
        assert!(scan.disc_type().is_none());
        assert!(scan.finish().is_empty());
        assert!(split_fields("").is_none());
    }

    #[test]
    fn empty_drive_slots_are_not_drives() {
        assert!(parse_drive(&fields(r#"DRV:1,256,999,0,"","","""#)).is_none());
        let drive = parse_drive(&fields(
            r#"DRV:0,2,999,12,"HL-DT-ST BD-RE","Movie, The","/dev/sr0""#,
        ))
        .unwrap();
        assert_eq!(drive.id, 0);
        assert_eq!(drive.name, "HL-DT-ST BD-RE");
        assert_eq!(drive.disc_label.as_deref(), Some("Movie, The"));
    }

    #[test]
    fn an_empty_disc_name_reads_as_no_disc() {
        let drive = parse_drive(&fields(r#"DRV:0,2,999,12,"BD-RE","","/dev/sr0""#)).unwrap();
        assert!(drive.disc_label.is_none());
    }

    #[test]
    fn titles_come_out_structured() {
        let mut scan = TitleScan::default();
        for line in [
            "TCOUNT:2",
            r#"CINFO:2,0,"SEASON_1_DISC_2""#,
            r#"TINFO:0,2,0,"Episode 1, Pilot""#,
            r#"TINFO:0,8,0,"12""#,
            r#"TINFO:0,9,0,"1:23:45""#,
            r#"TINFO:0,11,0,"13215223808""#,
            r#"TINFO:0,27,0,"title_t00.mkv""#,
            r#"SINFO:0,0,1,6201,"Video""#,
            r#"SINFO:0,0,6,0,"MPEG-2""#,
            r#"SINFO:0,0,19,0,"720x480""#,
            r#"SINFO:0,1,1,6202,"Audio""#,
            r#"SINFO:0,1,6,0,"DTS""#,
            r#"SINFO:0,1,40,0,"5.1""#,
            r#"SINFO:0,1,4,0,"English""#,
            r#"TINFO:1,27,0,"title_t01.mkv""#,
            r#"TINFO:1,9,0,"22:10""#,
            "garbage without a colon",
            "TINFO:1",
        ] {
            if let Some((prefix, f)) = split_fields(line) {
                scan.feed(prefix, &f);
            }
        }

        let titles = scan.finish();
        assert_eq!(titles.len(), 2);
        assert_eq!(titles[0].name, "Episode 1, Pilot");
        assert_eq!(titles[0].duration, Duration::from_secs(5025));
        assert_eq!(titles[0].size_bytes, 13_215_223_808);
        assert_eq!(titles[0].chapters, 12);
        assert_eq!(
            titles[0].tracks,
            vec!["Video MPEG-2 720x480", "Audio DTS 5.1 English"]
        );
        // No name attribute: the output file name stands in for it.
        assert_eq!(titles[1].name, "title_t01.mkv");
        assert_eq!(titles[1].duration, Duration::from_secs(1330));
        assert_eq!(titles[1].size_bytes, 0);
    }

    #[test]
    fn unparseable_durations_and_sizes_are_zero() {
        let mut scan = TitleScan::default();
        for line in [
            r#"TINFO:0,9,0,"unknown""#,
            r#"TINFO:0,11,0,"12.3 GB""#,
            r#"TINFO:0,8,0,"""#,
        ] {
            let (prefix, f) = split_fields(line).unwrap();
            scan.feed(prefix, &f);
        }
        let titles = scan.finish();
        assert_eq!(titles[0].duration, Duration::ZERO);
        assert_eq!(titles[0].size_bytes, 0);
        assert_eq!(titles[0].chapters, 0);
        assert_eq!(titles[0].name, "Title 0");
    }

    #[test]
    fn progress_is_the_overall_figure() {
        assert_eq!(
            progress_percent(&fields("PRGV:100,32768,65536")),
            Some(50.0)
        );
        assert_eq!(progress_percent(&fields("PRGV:0,0,65536")), Some(0.0));
        // A zero maximum is MakeMKV saying "not started", not 0%.
        assert_eq!(progress_percent(&fields("PRGV:0,0,0")), None);
        assert_eq!(progress_percent(&fields("PRGV:1")), None);
        assert_eq!(progress_percent(&fields(r#"PRGV:1,"x",2"#)), None);
        // Overshoot is clamped rather than reported as 130%.
        assert_eq!(progress_percent(&fields("PRGV:0,90,65")), Some(100.0));
    }

    #[test]
    fn the_output_name_is_read_only_for_the_ripped_title() {
        let line = fields(r#"TINFO:3,27,0,"title_t03.mkv""#);
        assert_eq!(output_file_name(&line, 3).as_deref(), Some("title_t03.mkv"));
        assert!(output_file_name(&line, 4).is_none());
        assert!(output_file_name(&fields(r#"TINFO:3,9,0,"1:23:45""#), 3).is_none());
    }
}
