//! Staging a ripped title: free space, disc recheck, a private directory per
//! rip, the rename that gives the file the disc's name, and the cleanup rules
//! for the temporary file afterwards.

use super::{DiscError, DiscSource, DiscTitle, RipProgress};
use crate::config::AppConfig;
use crate::queue::{EncodingJob, JobStatus};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tracing::{info, warn};

/// Prefix of every staging subdirectory. The sweeps delete only these.
const STAGING_PREFIX: &str = "rip-";

/// Free space required beyond the title's own size.
const SPACE_HEADROOM_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// How recently a staging directory must have been written to count as an
/// active rip belonging to another process. Thirty minutes covers makemkvcon's
/// read-retry loops on a damaged disc, which write nothing while they run.
pub const ACTIVE_RIP_WINDOW: Duration = Duration::from_mins(30);

/// The staging root: the configured directory, or one under the system temp
/// directory.
pub fn staging_root(config: &AppConfig) -> PathBuf {
    config
        .disc
        .staging_directory
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map_or_else(
            || std::env::temp_dir().join("av1converter-staging"),
            PathBuf::from,
        )
}

/// The directory encoded output goes to. A rip refuses to start without one:
/// `same_directory` would put the encode inside the staging directory that is
/// deleted once the encode finishes.
pub fn require_destination(config: &AppConfig) -> Result<PathBuf, DiscError> {
    let configured = config
        .output
        .output_directory
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .ok_or(DiscError::NoDestination)?;
    let path = PathBuf::from(configured);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(DiscError::NoDestination)
    }
}

/// Rip one title into a fresh staging directory and return the file it wrote,
/// named after the disc.
///
/// The steps before the rip are the ones that are worthless once forty minutes
/// of extraction have already run: free space, and the disc still being the
/// one that was scanned.
pub fn rip_to_staging(
    bin: &Path,
    config: &AppConfig,
    source: &DiscSource,
    title: &DiscTitle,
    on_progress: impl FnMut(RipProgress),
    cancel: &AtomicBool,
) -> Result<PathBuf, DiscError> {
    require_destination(config)?;

    let root = staging_root(config);
    std::fs::create_dir_all(&root).map_err(|e| {
        DiscError::Failed(format!(
            "could not create the staging directory {}: {e}",
            root.display()
        ))
    })?;

    // A title whose size did not parse (`size_bytes == 0`) is estimated from
    // its duration at the top Blu-ray video rate of 40 Mbit/s.
    let estimated_bytes = if title.size_bytes == 0 {
        title.duration.as_secs().saturating_mul(5 * 1024 * 1024)
    } else {
        title.size_bytes
    };
    let needed = estimated_bytes.saturating_add(SPACE_HEADROOM_BYTES);
    if available_bytes(&root).is_some_and(|free| free < needed) {
        return Err(DiscError::InsufficientSpace);
    }

    // `makemkvcon mkv` re-scans the disc, so a title id only means anything for
    // the disc physically in the drive. A folder on disk cannot be swapped.
    if let DiscSource::Drive(drive) = source {
        let present = super::list_drives(bin, cancel)?;
        if !present
            .iter()
            .any(|current| current.id == drive.id && current.disc_label == drive.disc_label)
        {
            return Err(DiscError::DiscChanged);
        }
    }

    let dir = create_staging_dir(&root)?;
    let ripped = match super::rip_title(bin, source, title.id, &dir, on_progress, cancel) {
        Ok(path) => path,
        Err(e) => {
            // A partial MKV is indistinguishable from a good one later.
            discard_dir(&dir);
            return Err(e);
        }
    };

    let named = dir.join(staged_name(source.label(), title.id));
    if let Err(e) = std::fs::rename(&ripped, &named) {
        warn!(
            "Could not rename {} to {}: {e}",
            ripped.display(),
            named.display()
        );
        return Ok(ripped);
    }
    Ok(named)
}

/// `<disc label>_t<NN>.mkv`, in place of `MakeMKV`'s `title_t00.mkv`.
fn staged_name(disc_label: Option<&str>, title_id: u32) -> String {
    format!("{}_t{title_id:02}.mkv", sanitize_label(disc_label))
}

/// A disc label reduced to one path component.
fn sanitize_label(label: Option<&str>) -> String {
    let cleaned: String = label
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')' | '\'' | ',') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let trimmed = cleaned.trim_matches([' ', '.', '_']);
    if trimmed.is_empty() {
        "disc".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Delete the staging file of every temporary job whose encode has finished.
///
/// | Status | Staging file |
/// |---|---|
/// | `Done`, `DoneWithVmaf`, `DoneVmafFailed` | deleted with its directory |
/// | `QualityWarning`, `Error`, `Skipped` | kept, its path still on the job |
pub fn cleanup_finished(jobs: &mut [EncodingJob]) {
    for job in jobs.iter_mut().filter(|job| job.temporary) {
        if !matches!(
            job.status,
            JobStatus::Done | JobStatus::DoneWithVmaf { .. } | JobStatus::DoneVmafFailed { .. }
        ) {
            continue;
        }
        discard_staged(&job.path);
        job.temporary = false;
        job.source_deleted = true;
    }
}

/// Delete the staging directory holding `file`.
pub fn discard_staged(file: &Path) {
    if let Some(dir) = file.parent().filter(|dir| is_staging_dir(dir)) {
        discard_dir(dir);
    }
}

/// Delete staging directories that no job points into and that nothing has
/// written to for `min_age`.
///
/// Runs at daemon startup, where a rip cut short by a kill has nothing left
/// tracking its file. Only subdirectories of the staging root are ever removed,
/// never the root itself.
pub fn sweep_orphans(config: &AppConfig, jobs: &[EncodingJob], min_age: Duration) {
    let root = staging_root(config);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };

    let live: HashSet<PathBuf> = jobs
        .iter()
        .filter(|job| job.temporary)
        .filter_map(|job| job.path.parent().map(Path::to_path_buf))
        .collect();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !is_staging_dir(&path) || live.contains(&path) {
            continue;
        }
        // A directory another process is still ripping into. Unreadable
        // timestamps count as recent and the directory stays.
        match last_written(&path) {
            Some(age) if age >= min_age => {
                info!("Removing orphaned staging directory {}", path.display());
                discard_dir(&path);
            }
            Some(_) => info!("Leaving active staging directory {}", path.display()),
            None => warn!(
                "Leaving staging directory {}: its timestamps are unreadable",
                path.display()
            ),
        }
    }
}

fn is_staging_dir(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(STAGING_PREFIX))
}

fn discard_dir(dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir) {
        warn!("Could not remove {}: {e}", dir.display());
    }
}

/// Time since the newest write anywhere in `dir`, itself included.
fn last_written(dir: &Path) -> Option<Duration> {
    let entries = std::fs::read_dir(dir).ok()?;
    let newest = entries
        .flatten()
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .chain(std::fs::metadata(dir).ok()?.modified().ok())
        .max()?;
    newest.elapsed().ok()
}

/// A directory under `root` that no other process can already hold.
fn create_staging_dir(root: &Path) -> Result<PathBuf, DiscError> {
    for _ in 0..8 {
        let random = crate::utils::random_hex(8).map_err(DiscError::Failed)?;
        let candidate = root.join(format!("{STAGING_PREFIX}{random}"));
        if crate::utils::create_private_dir(&candidate).is_ok() {
            return Ok(candidate);
        }
    }
    Err(DiscError::Failed(format!(
        "could not create a staging directory in {}",
        root.display()
    )))
}

/// Free bytes on the filesystem holding `path`.
///
/// The `statvfs` field widths differ per platform — `f_bavail` is 32-bit on
/// macOS and 64-bit on Linux — so one of the two conversions is always a no-op.
#[cfg(unix)]
#[allow(clippy::useless_conversion)]
fn available_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a NUL-terminated string and `stat` is a live
    // `statvfs` the call only writes to.
    if unsafe { libc::statvfs(c_path.as_ptr(), &raw mut stat) } != 0 {
        return None;
    }
    Some(u64::from(stat.f_bavail).saturating_mul(stat.f_frsize.into()))
}

/// Windows has no `statvfs`, and the check is a courtesy: `MakeMKV` still
/// reports its own out-of-space failure.
#[cfg(not(unix))]
fn available_bytes(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DiscConfig, OutputConfig};

    #[cfg(unix)]
    use super::super::testing::{Fake, fake_makemkvcon};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("av1c_staging_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config_with_root(root: &Path) -> AppConfig {
        AppConfig {
            disc: DiscConfig {
                staging_directory: Some(root.to_string_lossy().into_owned()),
                ..DiscConfig::default()
            },
            ..AppConfig::default()
        }
    }

    /// The drive and title the shared fake reports, as a caller holds them
    /// after a listing and a scan.
    #[cfg(unix)]
    fn disc(label: &str) -> (DiscSource, DiscTitle) {
        (
            DiscSource::Drive(super::super::DiscDrive {
                id: 0,
                name: "HL-DT-ST BD-RE WH16NS60".to_string(),
                disc_label: Some(label.to_string()),
            }),
            DiscTitle {
                id: 0,
                name: "Feature".to_string(),
                duration: Duration::from_mins(1),
                size_bytes: 1024,
                chapters: 4,
                tracks: Vec::new(),
            },
        )
    }

    /// A staging directory with one file in it, as a finished rip leaves it.
    fn staged_rip(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(format!("{STAGING_PREFIX}{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("DISC_t00.mkv");
        std::fs::write(&file, b"rip").unwrap();
        file
    }

    fn temporary_job(path: PathBuf, status: JobStatus) -> EncodingJob {
        let mut job = EncodingJob::new(path);
        job.temporary = true;
        job.status = status;
        job
    }

    #[test]
    fn labels_become_one_safe_path_component() {
        assert_eq!(
            staged_name(Some("Blade Runner, Final Cut"), 3),
            "Blade Runner, Final Cut_t03.mkv"
        );
        // Separators become underscores, and the leading ones are trimmed off.
        assert_eq!(
            staged_name(Some("../../etc/passwd"), 0),
            "etc_passwd_t00.mkv"
        );
        assert_eq!(staged_name(Some("  "), 12), "disc_t12.mkv");
        assert_eq!(staged_name(None, 1), "disc_t01.mkv");
        // Non-ASCII titles are names, not garbage to strip.
        assert_eq!(staged_name(Some("紅い眼鏡"), 2), "紅い眼鏡_t02.mkv");
        assert!(!staged_name(Some(&"x".repeat(500)), 0).contains(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn a_rip_needs_a_destination_outside_staging() {
        let root = scratch("destination");
        let mut config = config_with_root(&root);
        assert_eq!(require_destination(&config), Err(DiscError::NoDestination));

        config.output.output_directory = Some(String::new());
        assert_eq!(require_destination(&config), Err(DiscError::NoDestination));

        config.output.output_directory = Some(root.join("nope").to_string_lossy().into_owned());
        assert_eq!(require_destination(&config), Err(DiscError::NoDestination));

        let out = root.join("out");
        std::fs::create_dir_all(&out).unwrap();
        config.output.output_directory = Some(out.to_string_lossy().into_owned());
        assert_eq!(require_destination(&config), Ok(out));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// One case per row of the cleanup table.
    #[test]
    fn only_a_finished_encode_takes_its_staging_file_with_it() {
        let root = scratch("cleanup");
        let cases = [
            (JobStatus::Done, false),
            (JobStatus::DoneWithVmaf { score: 96.0 }, false),
            (
                JobStatus::DoneVmafFailed {
                    reason: "no libvmaf".to_string(),
                },
                false,
            ),
            (
                JobStatus::QualityWarning {
                    vmaf: 80.0,
                    threshold: 90.0,
                },
                true,
            ),
            (
                JobStatus::Error {
                    message: "ffmpeg died".to_string(),
                },
                true,
            ),
            (
                JobStatus::Skipped {
                    reason: "already AV1".to_string(),
                },
                true,
            ),
            (JobStatus::Ready, true),
        ];

        let mut jobs: Vec<EncodingJob> = cases
            .iter()
            .enumerate()
            .map(|(i, (status, _))| {
                temporary_job(staged_rip(&root, &i.to_string()), status.clone())
            })
            .collect();
        // A user's own file is never touched, whatever its status.
        let own = root.join("mine.mkv");
        std::fs::write(&own, b"mine").unwrap();
        jobs.push(EncodingJob::new(own.clone()));
        jobs.last_mut().unwrap().status = JobStatus::Done;

        cleanup_finished(&mut jobs);

        for (job, (status, kept)) in jobs.iter().zip(cases.iter()) {
            assert_eq!(
                job.path.exists(),
                *kept,
                "{status:?} should {} its staging file",
                if *kept { "keep" } else { "delete" }
            );
            assert_eq!(job.temporary, *kept);
            assert_eq!(job.source_deleted, !*kept);
            // A kept file is only useful if its directory is still there.
            assert_eq!(job.path.parent().unwrap().is_dir(), *kept);
        }
        assert!(own.exists(), "a job of the user's own is not staging");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Cleanup runs on every tick; the second one has nothing left to do.
    #[test]
    fn cleanup_is_repeatable() {
        let root = scratch("repeat");
        let mut jobs = vec![temporary_job(staged_rip(&root, "a"), JobStatus::Done)];
        cleanup_finished(&mut jobs);
        cleanup_finished(&mut jobs);
        assert!(!jobs[0].path.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_sweep_removes_only_unreferenced_staging_directories() {
        let root = scratch("sweep");
        let referenced = staged_rip(&root, "referenced");
        let orphan = staged_rip(&root, "orphan");
        let finished = staged_rip(&root, "finished");
        let stranger = root.join("not-a-rip");
        std::fs::create_dir_all(&stranger).unwrap();
        let loose = root.join("loose.mkv");
        std::fs::write(&loose, b"x").unwrap();

        let jobs = vec![
            temporary_job(referenced.clone(), JobStatus::Ready),
            // Already cleaned up: not temporary, so its directory is fair game.
            EncodingJob::new(finished.clone()),
        ];

        sweep_orphans(&config_with_root(&root), &jobs, Duration::ZERO);

        assert!(referenced.exists(), "a queued rip is not an orphan");
        assert!(!orphan.exists());
        assert!(!finished.exists());
        assert!(stranger.is_dir(), "only `rip-` directories are ours");
        assert!(loose.exists(), "the staging root itself is never emptied");
        assert!(root.is_dir());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Another process's rip in progress is left alone.
    #[test]
    fn the_sweep_leaves_an_active_rip_alone() {
        let root = scratch("active");
        let active = staged_rip(&root, "active");
        sweep_orphans(&config_with_root(&root), &[], ACTIVE_RIP_WINDOW);
        assert!(active.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An unset staging directory still resolves somewhere usable.
    #[test]
    fn the_staging_root_falls_back_to_the_temp_directory() {
        let root = staging_root(&AppConfig::default());
        assert!(root.starts_with(std::env::temp_dir()));
        assert_ne!(root, std::env::temp_dir());
    }

    #[cfg(unix)]
    #[test]
    fn free_space_is_reported_for_a_real_directory() {
        assert!(available_bytes(&std::env::temp_dir()).is_some());
        assert!(available_bytes(Path::new("/av1c/does/not/exist")).is_none());
    }

    /// The whole sequence: a private directory under the root, the disc's name
    /// on the file, and progress on the way.
    #[cfg(unix)]
    #[test]
    fn a_rip_lands_in_its_own_directory_under_the_disc_name() {
        let base = scratch("rip");
        let root = base.join("staging");
        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut config = config_with_root(&root);
        config.output.output_directory = Some(out.to_string_lossy().into_owned());

        let (source, title) = disc("THE_DISC");
        let bin = fake_makemkvcon(&base, &Fake::Rip);
        let mut seen = 0;
        let file = rip_to_staging(
            &bin,
            &config,
            &source,
            &title,
            |_| seen += 1,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(seen, 1);
        assert_eq!(file.file_name().unwrap(), "THE_DISC_t00.mkv");
        assert!(file.is_file());
        let dir = file.parent().unwrap();
        assert!(is_staging_dir(dir));
        assert_eq!(dir.parent().unwrap(), root);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A folder source never asks which disc is in the drive: the fake's drive
    /// listing fails, and the rip still lands.
    #[cfg(unix)]
    #[test]
    fn a_folder_rip_never_looks_at_the_drives() {
        let base = scratch("folder");
        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let root = base.join("staging");
        let mut config = config_with_root(&root);
        config.output.output_directory = Some(out.to_string_lossy().into_owned());

        let ripped = base.join("THE_DISC");
        std::fs::create_dir_all(ripped.join("BDMV")).unwrap();
        let source = DiscSource::folder(&ripped).unwrap();
        let bin = fake_makemkvcon(&base, &Fake::FolderScan);
        assert_eq!(
            super::super::list_drives(&bin, &AtomicBool::new(false)),
            Err(DiscError::NoDrive),
            "the fake has no drive to check against"
        );

        let (_, title) = disc("THE_DISC");
        let file = rip_to_staging(
            &bin,
            &config,
            &source,
            &title,
            |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(file.file_name().unwrap(), "THE_DISC_t00.mkv");
        assert!(file.is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A swapped disc invalidates the title ids the rip was asked for.
    #[cfg(unix)]
    #[test]
    fn a_different_disc_aborts_before_any_extraction() {
        let base = scratch("swapped");
        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let root = base.join("staging");
        let mut config = config_with_root(&root);
        config.output.output_directory = Some(out.to_string_lossy().into_owned());

        let (source, title) = disc("A_DIFFERENT_DISC");
        let bin = fake_makemkvcon(&base, &Fake::Rip);
        let result = rip_to_staging(
            &bin,
            &config,
            &source,
            &title,
            |_| {},
            &AtomicBool::new(false),
        );

        assert_eq!(result, Err(DiscError::DiscChanged));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A rip that fails takes its half-written file with it.
    #[cfg(unix)]
    #[test]
    fn a_failed_rip_leaves_no_partial_behind() {
        let base = scratch("partial");
        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let root = base.join("staging");
        let mut config = config_with_root(&root);
        config.output.output_directory = Some(out.to_string_lossy().into_owned());

        let (source, title) = disc("THE_DISC");
        let bin = fake_makemkvcon(&base, &Fake::RipFails);
        let result = rip_to_staging(
            &bin,
            &config,
            &source,
            &title,
            |_| {},
            &AtomicBool::new(false),
        );

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            0,
            "the staging directory of a failed rip is deleted"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A staged job's encode never lands in the directory that gets deleted.
    #[test]
    fn a_staged_job_encodes_outside_its_staging_directory() {
        let root = scratch("output");
        let file = staged_rip(&root, "out");
        let mut job = temporary_job(file.clone(), JobStatus::Ready);
        let config = OutputConfig {
            same_directory: true,
            output_directory: Some("/library".to_string()),
            ..OutputConfig::default()
        };

        job.generate_output_path(&config);
        let output = job.output_path.clone().unwrap();
        assert_eq!(output, PathBuf::from("/library/DISC_t00_av1.mkv"));
        assert!(!output.starts_with(&root));

        let _ = std::fs::remove_dir_all(&root);
    }
}
