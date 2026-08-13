//! Scanning and ripping off the UI thread, following the analysis worker
//! pattern: one thread, one channel, drained by the event loop.
//!
//! The worker never waits on whoever consumes its events. A title that has
//! finished extracting is announced and the next one starts immediately, so a
//! slow consumer — analysis, then an encode — cannot stall the drive.

use super::{DiscDrive, DiscError, DiscTitle, staging};
use crate::config::AppConfig;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::thread;

/// What a scan or a rip run reports back. `index` is the position in the
/// requested list, which is what the caller's queue is keyed on.
#[derive(Debug)]
pub enum DiscEvent {
    /// The disc was scanned.
    TitlesFound(super::DiscScan),
    /// Extraction progress for one title.
    Ripping { index: usize, progress: f64 },
    /// A title finished extracting; its file is ready to analyze.
    TitleReady { index: usize, path: PathBuf },
    /// The run stopped here. Titles already extracted stay.
    Error { index: usize, error: DiscError },
    /// The run stopped because it was asked to.
    Cancelled,
    /// Every requested title was extracted.
    Finished,
}

/// Scan the disc in `drive` and report its titles.
pub fn spawn_scan(bin: PathBuf, drive: u32, cancel: &Arc<AtomicBool>, tx: Sender<DiscEvent>) {
    let cancel = cancel.clone();
    thread::spawn(move || {
        let scanned = std::panic::catch_unwind(AssertUnwindSafe(|| {
            super::scan_titles(&bin, drive, &cancel)
        }));
        let _ = tx.send(match scanned {
            Ok(Ok(scan)) => DiscEvent::TitlesFound(scan),
            Ok(Err(DiscError::Cancelled)) => DiscEvent::Cancelled,
            Ok(Err(error)) => DiscEvent::Error { index: 0, error },
            Err(_) => DiscEvent::Error {
                index: 0,
                error: DiscError::Failed("the disc scan panicked".to_string()),
            },
        });
    });
}

/// Extract `titles` one after another, reporting each file as it lands.
///
/// One rip at a time: a single optical drive cannot usefully extract two
/// titles at once. A title that fails ends the run — a disc that stopped
/// reading rarely reads the next title either.
pub fn spawn_rips(
    bin: PathBuf,
    config: AppConfig,
    drive: DiscDrive,
    titles: Vec<DiscTitle>,
    cancel: &Arc<AtomicBool>,
    tx: Sender<DiscEvent>,
) {
    let cancel = cancel.clone();
    thread::spawn(move || {
        for (index, title) in titles.iter().enumerate() {
            let progress_tx = tx.clone();
            // A panic here ends the run with an error rather than leaving the
            // caller waiting on a thread that is already gone.
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                staging::rip_to_staging(
                    &bin,
                    &config,
                    &drive,
                    title,
                    |progress| {
                        let _ = progress_tx.send(DiscEvent::Ripping {
                            index,
                            progress: progress.percent,
                        });
                    },
                    &cancel,
                )
            }));

            match result {
                Ok(Ok(path)) => {
                    let _ = tx.send(DiscEvent::TitleReady { index, path });
                }
                Ok(Err(DiscError::Cancelled)) => {
                    let _ = tx.send(DiscEvent::Cancelled);
                    return;
                }
                Ok(Err(error)) => {
                    let _ = tx.send(DiscEvent::Error { index, error });
                    return;
                }
                Err(_) => {
                    let _ = tx.send(DiscEvent::Error {
                        index,
                        error: DiscError::Failed("the rip panicked".to_string()),
                    });
                    return;
                }
            }
        }
        let _ = tx.send(DiscEvent::Finished);
    });
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::testing::{Fake, fake_makemkvcon};
    use super::*;
    use crate::config::{DiscConfig, OutputConfig};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Generous enough that a regression fails on the timeout instead of
    /// hanging, and slack enough not to flake on a loaded CI machine.
    const TIMEOUT: Duration = Duration::from_secs(30);

    fn two_titles() -> Vec<DiscTitle> {
        (0..2)
            .map(|id| DiscTitle {
                id,
                name: format!("Title {id}"),
                duration: Duration::from_mins(1),
                size_bytes: 1024,
                chapters: 1,
                tracks: Vec::new(),
            })
            .collect()
    }

    /// A consumer that sits on the first finished title — an encode that has
    /// not returned — must not stop the drive extracting the next one.
    #[test]
    fn a_blocked_consumer_does_not_stall_the_next_rip() {
        let base = std::env::temp_dir().join("av1c_disc_worker_interleave");
        let _ = std::fs::remove_dir_all(&base);
        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let bin = fake_makemkvcon(&base, &Fake::SlowRip);

        let config = AppConfig {
            disc: DiscConfig {
                staging_directory: Some(base.join("staging").to_string_lossy().into_owned()),
                ..DiscConfig::default()
            },
            output: OutputConfig {
                output_directory: Some(out.to_string_lossy().into_owned()),
                ..OutputConfig::default()
            },
            ..AppConfig::default()
        };
        let drive = DiscDrive {
            id: 0,
            name: "HL-DT-ST BD-RE WH16NS60".to_string(),
            disc_label: Some("THE_DISC".to_string()),
        };

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        spawn_rips(bin, config, drive, two_titles(), &cancel, tx);

        // Hold the first finished title, as a slow encode would.
        let mut ready = Vec::new();
        while ready.is_empty() {
            match rx.recv_timeout(TIMEOUT).expect("the first title") {
                DiscEvent::TitleReady { index, .. } => ready.push(index),
                DiscEvent::Error { error, .. } => panic!("rip failed: {error}"),
                _ => {}
            }
        }
        assert_eq!(ready, vec![0]);

        // Still holding it: the worker must be extracting the second title.
        let mut second_started = false;
        while !second_started {
            match rx
                .recv_timeout(TIMEOUT)
                .expect("progress on the second title")
            {
                DiscEvent::Ripping { index: 1, .. } => second_started = true,
                DiscEvent::Error { error, .. } => panic!("rip failed: {error}"),
                DiscEvent::Finished => panic!("finished without reporting the second rip"),
                _ => {}
            }
        }

        // Releasing the consumer, both titles complete and the run ends.
        let mut finished = false;
        while !finished {
            match rx.recv_timeout(TIMEOUT).expect("the run to finish") {
                DiscEvent::TitleReady { index, .. } => ready.push(index),
                DiscEvent::Finished => finished = true,
                DiscEvent::Error { error, .. } => panic!("rip failed: {error}"),
                _ => {}
            }
        }
        assert_eq!(ready, vec![0, 1]);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Cancelling mid-rip is reported as a cancellation, not as a failure, and
    /// the partial file goes with it.
    #[test]
    fn cancelling_reports_cancelled_and_leaves_nothing_behind() {
        let base = std::env::temp_dir().join("av1c_disc_worker_cancel");
        let _ = std::fs::remove_dir_all(&base);
        let out = base.join("out");
        let staging = base.join("staging");
        std::fs::create_dir_all(&out).unwrap();
        let bin = fake_makemkvcon(&base, &Fake::SlowRip);

        let config = AppConfig {
            disc: DiscConfig {
                staging_directory: Some(staging.to_string_lossy().into_owned()),
                ..DiscConfig::default()
            },
            output: OutputConfig {
                output_directory: Some(out.to_string_lossy().into_owned()),
                ..OutputConfig::default()
            },
            ..AppConfig::default()
        };
        let drive = DiscDrive {
            id: 0,
            name: "HL-DT-ST BD-RE WH16NS60".to_string(),
            disc_label: Some("THE_DISC".to_string()),
        };

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        spawn_rips(bin, config, drive, two_titles(), &cancel, tx);

        let mut cancelled = false;
        while !cancelled {
            match rx.recv_timeout(TIMEOUT).expect("an event") {
                // Cancel from inside the run, at the first sign of progress.
                DiscEvent::Ripping { .. } => cancel.store(true, Ordering::Relaxed),
                DiscEvent::Cancelled => cancelled = true,
                other => panic!("expected a cancellation, got {other:?}"),
            }
        }
        assert_eq!(
            std::fs::read_dir(&staging).map_or(0, Iterator::count),
            0,
            "the partial rip is deleted with its staging directory"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
