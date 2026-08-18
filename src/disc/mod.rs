//! Disc ripping backend: a thin wrapper around the `makemkvcon` CLI.
//!
//! Everything goes through `MakeMKV`'s robot mode (`-r`) and its opaque drive
//! ids; no device path is ever parsed.

pub mod robot;
pub mod staging;
#[cfg(all(test, unix))]
pub mod testing;
pub mod worker;

use crate::config::AppConfig;
use robot::TitleScan;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// File names the binary goes by, most specific first.
#[cfg(windows)]
const BINARY_NAMES: &[&str] = &["makemkvcon64.exe", "makemkvcon.exe"];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["makemkvcon"];

/// Install locations to fall back to when the binary is not on `PATH`: the
/// macOS app bundle and the Windows install directory. On Linux `makemkvcon`
/// installs onto `PATH`.
#[cfg(target_os = "macos")]
const INSTALL_PATHS: &[&str] = &["/Applications/MakeMKV.app/Contents/MacOS/makemkvcon"];
#[cfg(windows)]
const INSTALL_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\MakeMKV\makemkvcon64.exe",
    r"C:\Program Files (x86)\MakeMKV\makemkvcon.exe",
];
#[cfg(not(any(target_os = "macos", windows)))]
const INSTALL_PATHS: &[&str] = &[];

/// Shortest title a scan reports; `MakeMKV`'s own default hides short episodes
/// and extras. Scanning and ripping pass the same value — the title index of
/// `mkv disc:N <title>` indexes the list this filter produced.
const MIN_TITLE_LENGTH_SECS: u32 = 60;

/// How often the output loop checks the cancel flag while `MakeMKV` is quiet.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Upper bound on retained `MSG` lines; a healthy run prints a handful.
const MAX_MESSAGES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscDrive {
    pub id: u32,
    pub name: String,
    /// `None` when the drive is empty, or the disc has no volume name.
    pub disc_label: Option<String>,
}

/// Where `MakeMKV` reads a disc from: a drive, or a ripped folder or ISO image
/// on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscSource {
    Drive(DiscDrive),
    Folder(PathBuf),
}

impl DiscSource {
    /// A ripped disc folder or an ISO image on disk.
    ///
    /// A folder is one named `VIDEO_TS` or `BDMV`, or one holding either.
    pub fn folder(path: impl Into<PathBuf>) -> Result<Self, DiscError> {
        let path = path.into();
        if path.to_str().is_none() {
            return Err(DiscError::Failed(format!(
                "{} is not a valid UTF-8 path",
                path.display()
            )));
        }

        let holds = |name: &str| {
            path.file_name()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(name))
                || path.join(name).is_dir()
        };
        let usable = if is_iso(&path) {
            path.is_file()
        } else {
            path.is_dir() && (holds("VIDEO_TS") || holds("BDMV"))
        };
        if usable {
            Ok(DiscSource::Folder(path))
        } else {
            Err(DiscError::NotADiscFolder)
        }
    }

    /// The source argument `makemkvcon` takes.
    pub fn to_arg(&self) -> String {
        match self {
            DiscSource::Drive(drive) => format!("disc:{}", drive.id),
            DiscSource::Folder(path) => {
                let scheme = if is_iso(path) { "iso" } else { "file" };
                format!("{scheme}:{}", path.display())
            }
        }
    }

    /// What to call the disc: its volume name, or the name of the folder or
    /// image it was read from.
    pub fn label(&self) -> Option<&str> {
        match self {
            DiscSource::Drive(drive) => drive.disc_label.as_deref(),
            DiscSource::Folder(path) => path.file_name().and_then(std::ffi::OsStr::to_str),
        }
    }
}

fn is_iso(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("iso"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscTitle {
    pub id: u32,
    pub name: String,
    pub duration: Duration,
    pub size_bytes: u64,
    pub chapters: u32,
    /// One human-readable line per stream, for the title screen.
    pub tracks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RipProgress {
    pub title_id: u32,
    pub percent: f64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscError {
    NotInstalled,
    NoDrive,
    DriveEmpty,
    PermissionDenied,
    /// Blu-ray decryption needs a licence or the free beta key, and the beta
    /// key expires every couple of months.
    KeyExpired,
    UnreadableDisc,
    InsufficientSpace,
    /// The disc in the drive is no longer the one that was scanned, so the
    /// title ids no longer mean anything.
    DiscChanged,
    /// No output directory is configured for the encode to be written to.
    NoDestination,
    /// The path given is neither a folder holding `VIDEO_TS` or `BDMV` nor an
    /// ISO image.
    NotADiscFolder,
    Cancelled,
    /// Whatever `MakeMKV` said, when none of the above fits.
    Failed(String),
}

impl std::fmt::Display for DiscError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscError::NotInstalled => write!(
                f,
                "MakeMKV was not found. Install it, or set disc.makemkvcon_path in the config file"
            ),
            DiscError::NoDrive => write!(f, "No optical drive was found"),
            DiscError::DriveEmpty => write!(f, "The drive is empty"),
            DiscError::PermissionDenied => {
                write!(
                    f,
                    "The optical drive could not be opened: permission denied"
                )
            }
            DiscError::KeyExpired => write!(
                f,
                "MakeMKV's Blu-ray key has expired. DVDs still work; Blu-ray needs a purchased licence or a refreshed beta key from the MakeMKV forum"
            ),
            DiscError::UnreadableDisc => write!(
                f,
                "The disc could not be read: it may be damaged, unsupported, or still loading"
            ),
            DiscError::InsufficientSpace => write!(f, "Not enough free space for this title"),
            DiscError::DiscChanged => write!(
                f,
                "The disc in the drive is not the one that was scanned. Scan it again"
            ),
            DiscError::NoDestination => write!(
                f,
                "Set an output directory before ripping: the encode cannot be written into the staging directory"
            ),
            DiscError::NotADiscFolder => write!(
                f,
                "That is not a ripped disc: pick a folder holding VIDEO_TS or BDMV, or an ISO image"
            ),
            DiscError::Cancelled => write!(f, "Cancelled"),
            DiscError::Failed(message) => write!(f, "MakeMKV failed: {message}"),
        }
    }
}

impl std::error::Error for DiscError {}

impl DiscError {
    /// The same failure in the user's language. The catch-all keeps
    /// `MakeMKV`'s own words, behind a translated prefix.
    pub fn message(&self, lang: crate::i18n::Language) -> String {
        use crate::i18n::{Msg, t};
        let key = match self {
            DiscError::NotInstalled => Msg::DiscNotInstalled,
            DiscError::NoDrive => Msg::DiscNoDrive,
            DiscError::DriveEmpty => Msg::DiscDriveEmpty,
            DiscError::PermissionDenied => Msg::DiscPermissionDenied,
            DiscError::KeyExpired => Msg::DiscKeyExpired,
            DiscError::UnreadableDisc => Msg::DiscUnreadable,
            DiscError::InsufficientSpace => Msg::DiscInsufficientSpace,
            DiscError::DiscChanged => Msg::DiscChanged,
            DiscError::NoDestination => Msg::DiscNoDestination,
            DiscError::NotADiscFolder => Msg::DiscNotADiscFolder,
            DiscError::Cancelled => Msg::Cancelled,
            DiscError::Failed(message) => {
                return format!("{}: {message}", t(lang, Msg::DiscFailedPrefix));
            }
        };
        t(lang, key).to_string()
    }
}

/// Locate `makemkvcon`: the configured path, then `PATH`, then
/// [`INSTALL_PATHS`]. A binary that is nowhere reports
/// [`DiscError::NotInstalled`].
pub fn find_makemkvcon(config: &AppConfig) -> Result<PathBuf, DiscError> {
    if let Some(configured) = config
        .disc
        .makemkvcon_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let path = PathBuf::from(configured);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(DiscError::NotInstalled)
        };
    }

    if let Some(found) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .flat_map(|dir| BINARY_NAMES.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file())
    }) {
        return Ok(found);
    }

    INSTALL_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .ok_or(DiscError::NotInstalled)
}

/// Every drive `MakeMKV` can see, with the label of whatever is loaded.
pub fn list_drives(bin: &Path, cancel: &AtomicBool) -> Result<Vec<DiscDrive>, DiscError> {
    let mut drives = Vec::new();
    let run = run_robot(
        bin,
        &["-r", "--cache=1", "info", "disc:9999"],
        cancel,
        |prefix, fields| {
            if prefix == "DRV"
                && let Some(drive) = robot::parse_drive(fields)
            {
                drives.push(drive);
            }
        },
    )?;

    // `disc:9999` lists drives and always exits non-zero; the drives parsed
    // out of it are the success signal.
    if drives.is_empty() {
        return Err(failure(&run.messages, DiscError::NoDrive));
    }
    Ok(drives)
}

/// What one `info disc:N` run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscScan {
    /// `MakeMKV`'s own words for the medium: "Blu-ray disc", "DVD disc".
    pub disc_type: Option<String>,
    pub titles: Vec<DiscTitle>,
}

/// Scan `source` and return its titles.
pub fn scan_titles(
    bin: &Path,
    source: &DiscSource,
    cancel: &AtomicBool,
) -> Result<DiscScan, DiscError> {
    let minlength = format!("--minlength={MIN_TITLE_LENGTH_SECS}");
    let disc = source.to_arg();
    let mut scan = TitleScan::default();
    let run = run_robot(
        bin,
        &["-r", "--cache=1", &minlength, "info", &disc],
        cancel,
        |prefix, fields| scan.feed(prefix, fields),
    )?;

    let disc_type = scan.disc_type();
    let titles = scan.finish();
    if titles.is_empty() {
        return Err(failure(&run.messages, DiscError::DriveEmpty));
    }
    if !run.success {
        return Err(failure(&run.messages, DiscError::UnreadableDisc));
    }
    Ok(DiscScan { disc_type, titles })
}

/// Extract one title into `dest`, reporting progress until it finishes, fails
/// or is cancelled. Returns the file `MakeMKV` wrote.
///
/// A partially written file is left in `dest` for its owner to deal with.
pub fn rip_title(
    bin: &Path,
    source: &DiscSource,
    title: u32,
    dest: &Path,
    mut on_progress: impl FnMut(RipProgress),
    cancel: &AtomicBool,
) -> Result<PathBuf, DiscError> {
    let dest_arg = dest
        .to_str()
        .ok_or_else(|| DiscError::Failed("destination path is not valid UTF-8".to_string()))?;
    let minlength = format!("--minlength={MIN_TITLE_LENGTH_SECS}");
    let disc = source.to_arg();
    let title_arg = title.to_string();

    let mut message = String::new();
    let mut output_name = None;
    let run = run_robot(
        bin,
        &[
            "-r",
            "--progress=-same",
            &minlength,
            "mkv",
            &disc,
            &title_arg,
            dest_arg,
        ],
        cancel,
        |prefix, fields| match prefix {
            "PRGC" => {
                if let Some(name) = fields.get(2) {
                    message.clone_from(name);
                }
            }
            "PRGV" => {
                if let Some(percent) = robot::progress_percent(fields) {
                    on_progress(RipProgress {
                        title_id: title,
                        percent,
                        message: message.clone(),
                    });
                }
            }
            "TINFO" => {
                if let Some(name) = robot::output_file_name(fields, title) {
                    output_name = Some(name);
                }
            }
            _ => {}
        },
    )?;

    if !run.success {
        return Err(failure(&run.messages, DiscError::UnreadableDisc));
    }
    ripped_file(dest, output_name.as_deref())
}

/// The file the rip produced: the name `MakeMKV` reported, stripped to a bare
/// file name since it comes from disc metadata, and otherwise the only MKV in
/// `dest`.
fn ripped_file(dest: &Path, reported: Option<&str>) -> Result<PathBuf, DiscError> {
    if let Some(name) = reported.map(Path::new).and_then(Path::file_name) {
        let path = dest.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }

    let mut produced = std::fs::read_dir(dest)
        .map_err(|e| DiscError::Failed(format!("could not read {}: {e}", dest.display())))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mkv"))
        });
    match (produced.next(), produced.next()) {
        (Some(path), None) => Ok(path),
        _ => Err(DiscError::Failed(
            "the rip reported success but produced no single MKV".to_string(),
        )),
    }
}

struct RobotRun {
    success: bool,
    messages: Vec<String>,
}

/// Run `makemkvcon` and stream its robot output to `on_line`, killing the child
/// as soon as `cancel` is set.
fn run_robot(
    bin: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    mut on_line: impl FnMut(&str, &[String]),
) -> Result<RobotRun, DiscError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(DiscError::Cancelled);
    }

    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Robot output all goes to stdout.
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DiscError::NotInstalled
            } else {
                DiscError::Failed(format!("could not run {}: {e}", bin.display()))
            }
        })?;

    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            // Volume names are not always UTF-8; bad bytes are replaced rather
            // than ending the read.
            let text = String::from_utf8_lossy(&line).trim_end().to_string();
            if tx.send(text).is_err() {
                break;
            }
        }
    });

    let mut messages = Vec::new();
    let mut cancelled = false;
    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(line) => {
                if let Some((prefix, fields)) = robot::split_fields(&line) {
                    if prefix == "MSG"
                        && messages.len() < MAX_MESSAGES
                        && let Some(text) = fields.get(3)
                    {
                        messages.push(text.clone());
                    }
                    on_line(prefix, &fields);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            let _ = child.kill();
            break;
        }
    }

    let status = child
        .wait()
        .map_err(|e| DiscError::Failed(format!("makemkvcon could not be waited for: {e}")))?;
    if cancelled {
        // The reader is left to finish on its own: a child of the killed
        // process can still hold the write end of the pipe, and cancellation
        // must not wait for it.
        return Err(DiscError::Cancelled);
    }
    let _ = reader.join();
    Ok(RobotRun {
        success: status.success(),
        messages,
    })
}

/// Turn `MakeMKV`'s messages into an error, falling back to `default` when it
/// said nothing at all.
fn failure(messages: &[String], default: DiscError) -> DiscError {
    classify(messages).unwrap_or_else(|| {
        messages
            .last()
            .map_or(default, |last| DiscError::Failed(last.clone()))
    })
}

/// Recognise the failures that have their own wording.
///
/// Keyword matching on `MakeMKV`'s English message text, its message codes
/// moving between versions.
fn classify(messages: &[String]) -> Option<DiscError> {
    for message in messages {
        let text = message.to_lowercase();
        let expired = text.contains("expire");
        let licence = ["key", "registration", "evaluation", "licence", "license"]
            .iter()
            .any(|word| text.contains(word));
        if expired && licence {
            return Some(DiscError::KeyExpired);
        }
        if text.contains("usable optical drive") || text.contains("no optical drive") {
            return Some(DiscError::NoDrive);
        }
        if text.contains("permission denied")
            || text.contains("access denied")
            || text.contains("operation not permitted")
        {
            return Some(DiscError::PermissionDenied);
        }
        if text.contains("no disc")
            || text.contains("media not present")
            || text.contains("drive is empty")
        {
            return Some(DiscError::DriveEmpty);
        }
        if text.contains("no space") || text.contains("not enough space") {
            return Some(DiscError::InsufficientSpace);
        }
        if text.contains("failed to open disc")
            || text.contains("scsi error")
            || text.contains("read error")
            || text.contains("hash check failed")
        {
            return Some(DiscError::UnreadableDisc);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use super::testing::{Fake, fake_makemkvcon, scratch};

    /// The first drive of the shared fake's listing.
    #[cfg(unix)]
    fn drive_source() -> DiscSource {
        DiscSource::Drive(DiscDrive {
            id: 0,
            name: "HL-DT-ST BD-RE WH16NS60".to_string(),
            disc_label: Some("THE_DISC".to_string()),
        })
    }

    #[test]
    fn a_configured_path_that_is_not_there_reads_as_not_installed() {
        let mut config = AppConfig::default();
        config.disc.makemkvcon_path = Some("/nowhere/makemkvcon".to_string());
        assert_eq!(find_makemkvcon(&config), Err(DiscError::NotInstalled));
    }

    #[cfg(unix)]
    #[test]
    fn a_configured_path_wins_over_path_lookup() {
        let bin = fake_makemkvcon(&scratch("configured"), &Fake::Rip);
        let mut config = AppConfig::default();
        config.disc.makemkvcon_path = Some(bin.to_string_lossy().into_owned());
        assert_eq!(find_makemkvcon(&config), Ok(bin));
    }

    #[cfg(unix)]
    #[test]
    fn drives_survive_the_non_zero_exit_of_the_listing_trick() {
        let bin = fake_makemkvcon(&scratch("drives"), &Fake::Bluray);
        let drives = list_drives(&bin, &AtomicBool::new(false)).unwrap();
        // The empty slot in the listing is not a drive.
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].disc_label.as_deref(), Some("THE_DISC"));
    }

    /// A DVD and a Blu-ray, each parsed from the shape `makemkvcon` reports.
    #[cfg(unix)]
    #[test]
    fn a_dvd_and_a_blu_ray_scan_come_out_structured() {
        let dvd = scan_titles(
            &fake_makemkvcon(&scratch("dvd"), &Fake::Dvd),
            &drive_source(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(dvd.disc_type.as_deref(), Some("DVD disc"));
        assert_eq!(dvd.titles.len(), 5, "four episodes and an extra");
        assert_eq!(dvd.titles[0].name, "Episode 5, The Return");
        assert_eq!(dvd.titles[0].duration, Duration::from_secs(2652));
        assert_eq!(dvd.titles[0].chapters, 6);
        assert_eq!(
            dvd.titles[0].tracks,
            vec![
                "Video MPEG-2 720x576",
                "Audio DD 5.1 English",
                "Subtitles English",
            ]
        );
        assert_eq!(dvd.titles[4].name, "Behind the Scenes");

        let bluray = scan_titles(
            &fake_makemkvcon(&scratch("bluray"), &Fake::Bluray),
            &drive_source(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(bluray.disc_type.as_deref(), Some("Blu-ray disc"));
        assert_eq!(bluray.titles.len(), 2);
        assert_eq!(bluray.titles[0].name, "Blade Runner, The \"Final\" Cut");
        assert_eq!(bluray.titles[0].size_bytes, 29_715_223_808);
        assert_eq!(bluray.titles[1].name, "Commentary");
    }

    /// A folder on disk is scanned through `file:`, not through a drive id.
    #[cfg(unix)]
    #[test]
    fn a_folder_is_scanned_as_a_source_of_its_own() {
        let dir = scratch("folder_scan");
        let bin = fake_makemkvcon(&dir, &Fake::FolderScan);
        let disc = dir.join("Blade Runner");
        std::fs::create_dir_all(disc.join("BDMV")).unwrap();

        let source = DiscSource::folder(&disc).unwrap();
        let scan = scan_titles(&bin, &source, &AtomicBool::new(false)).unwrap();
        assert_eq!(scan.titles.len(), 2);
        assert_eq!(scan.titles[0].name, "Blade Runner, The \"Final\" Cut");

        let argv = std::fs::read_to_string(dir.join("argv")).unwrap();
        assert!(
            argv.contains(&format!("file:{}", disc.display())),
            "the folder went to makemkvcon as a file: source, got {argv}"
        );
        assert_eq!(source.label(), Some("Blade Runner"));
    }

    /// Only a folder MakeMKV can actually read is accepted, and the file name
    /// decides `file:` from `iso:`.
    #[cfg(unix)]
    #[test]
    fn a_disc_folder_is_told_apart_from_any_other_directory() {
        let dir = scratch("folder_kinds");
        let plain = dir.join("holiday photos");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(
            DiscSource::folder(&plain),
            Err(DiscError::NotADiscFolder),
            "a directory with no disc structure is not a disc"
        );
        assert_eq!(
            DiscSource::folder(dir.join("nowhere")),
            Err(DiscError::NotADiscFolder)
        );

        // A folder holding the disc structure, and the structure folder itself.
        let dvd = dir.join("SEASON_1");
        std::fs::create_dir_all(dvd.join("VIDEO_TS")).unwrap();
        assert_eq!(DiscSource::folder(&dvd).unwrap().to_arg(), {
            format!("file:{}", dvd.display())
        });
        assert!(DiscSource::folder(dvd.join("VIDEO_TS")).is_ok());
        let bluray = dir.join("THE_DISC");
        std::fs::create_dir_all(bluray.join("BDMV")).unwrap();
        assert!(DiscSource::folder(&bluray).is_ok());

        let iso = dir.join("Movie.ISO");
        std::fs::write(&iso, b"not really an image").unwrap();
        let source = DiscSource::folder(&iso).unwrap();
        assert_eq!(source.to_arg(), format!("iso:{}", iso.display()));
        assert_eq!(source.label(), Some("Movie.ISO"));
        // An ISO that is not there is no more usable than a plain directory.
        assert_eq!(
            DiscSource::folder(dir.join("missing.iso")),
            Err(DiscError::NotADiscFolder)
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_expired_key_and_a_closed_drive_are_told_apart() {
        assert_eq!(
            scan_titles(
                &fake_makemkvcon(&scratch("expired"), &Fake::ExpiredKey),
                &drive_source(),
                &AtomicBool::new(false)
            ),
            Err(DiscError::KeyExpired)
        );
        assert_eq!(
            list_drives(
                &fake_makemkvcon(&scratch("denied"), &Fake::PermissionDenied),
                &AtomicBool::new(false)
            ),
            Err(DiscError::PermissionDenied)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_rip_reports_progress_and_returns_the_file_it_wrote() {
        let dest = scratch("rip");
        let bin = fake_makemkvcon(&dest, &Fake::Rip);
        let mut seen = Vec::new();
        let file = rip_title(
            &bin,
            &drive_source(),
            0,
            &dest,
            |progress| seen.push((progress.percent, progress.message)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(file, dest.join("title_t00.mkv"));
        assert_eq!(seen, vec![(100.0, "Saving to MKV file".to_string())]);
    }

    /// A rip that stops partway reports why, rather than the file it half
    /// wrote.
    #[cfg(unix)]
    #[test]
    fn a_rip_that_fails_partway_reports_the_failure() {
        let dest = scratch("rip_fails");
        let bin = fake_makemkvcon(&dest, &Fake::RipFails);
        assert_eq!(
            rip_title(
                &bin,
                &drive_source(),
                0,
                &dest,
                |_| {},
                &AtomicBool::new(false)
            ),
            Err(DiscError::UnreadableDisc)
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_kills_the_rip_instead_of_waiting_for_it() {
        let dest = scratch("cancel");
        let bin = fake_makemkvcon(&dest, &Fake::RipHangs);
        let cancel = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let result = rip_title(
            &bin,
            &drive_source(),
            0,
            &dest,
            |_| cancel.store(true, Ordering::Relaxed),
            &cancel,
        );
        assert_eq!(result, Err(DiscError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    /// Every failure has its own translated wording, and the catch-all still
    /// carries what `MakeMKV` said.
    #[test]
    fn every_failure_reaches_the_user_in_its_own_words() {
        use crate::i18n::Language;

        let cases = [
            DiscError::NotInstalled,
            DiscError::NoDrive,
            DiscError::DriveEmpty,
            DiscError::PermissionDenied,
            DiscError::KeyExpired,
            DiscError::UnreadableDisc,
            DiscError::InsufficientSpace,
            DiscError::DiscChanged,
            DiscError::NoDestination,
            DiscError::NotADiscFolder,
            DiscError::Cancelled,
        ];

        let mut seen = std::collections::HashSet::new();
        for error in &cases {
            for &lang in &Language::ALL {
                let message = error.message(lang);
                assert!(!message.trim().is_empty(), "{error:?} is empty in {lang:?}");
            }
            assert!(
                seen.insert(error.message(Language::English)),
                "{error:?} shares its wording with another failure"
            );
        }

        let raw = DiscError::Failed("SCSI error 3:11:05".to_string());
        assert!(
            raw.message(Language::Italian)
                .contains("SCSI error 3:11:05")
        );
        assert_ne!(
            raw.message(Language::Italian),
            raw.message(Language::English)
        );
    }

    #[test]
    fn messages_are_classified_before_they_are_quoted_verbatim() {
        assert_eq!(
            classify(&["The program can't find any usable optical drives".to_string()]),
            Some(DiscError::NoDrive)
        );
        assert_eq!(
            classify(&["Failed to open disc".to_string()]),
            Some(DiscError::UnreadableDisc)
        );
        assert_eq!(
            classify(&["Error 'Permission denied' opening /dev/sr0".to_string()]),
            Some(DiscError::PermissionDenied)
        );
        assert_eq!(classify(&["Copy complete.".to_string()]), None);
        assert_eq!(
            failure(&["Copy complete.".to_string()], DiscError::DriveEmpty),
            DiscError::Failed("Copy complete.".to_string())
        );
        assert_eq!(failure(&[], DiscError::DriveEmpty), DiscError::DriveEmpty);
    }
}
