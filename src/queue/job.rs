use crate::analyzer::{DvMode, VideoMetadata};
use crate::config::{AudioConfig, TrackPresetConfig};
use crate::tracks::{AudioTrack, SubtitleTrack, TrackSelection};
use std::collections::HashSet;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::warn;

/// Filesystem identity captured while a source is analyzed. The encoder
/// re-checks it before doing any work and before an automatic source deletion,
/// so a file replaced at the same path is detected.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceIdentity {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl SourceIdentity {
    pub fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        std::fs::metadata(path).map(|metadata| Self::from_metadata(&metadata))
    }

    pub fn matches_path(&self, path: impl AsRef<Path>) -> bool {
        Self::from_path(path).is_ok_and(|current| *self == current)
    }

    pub fn size_bytes(&self) -> u64 {
        self.len
    }
}

/// Status of a job in the encoding queue
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum JobStatus {
    /// Waiting to be processed
    Pending,
    /// Being extracted from a disc. The percentage is not persisted: a
    /// reloaded `Ripping` job has no file to carry on from.
    Ripping {
        #[serde(skip)]
        progress: f64,
    },
    /// Being analyzed via ffprobe
    Analyzing,
    /// Waiting for track configuration
    AwaitingConfig,
    /// Ready to encode
    Ready,
    /// Currently encoding. The percentage is not persisted: a reloaded
    /// `Encoding` job restarts from zero.
    Encoding {
        #[serde(skip)]
        progress: f64,
    },
    /// Running VMAF quality check after encoding
    Verifying,
    /// Successfully encoded (no VMAF run)
    Done,
    /// Encoded with VMAF score meeting threshold
    DoneWithVmaf { score: f64 },
    /// Encoded successfully but VMAF check could not run
    DoneVmafFailed { reason: String },
    /// Skipped (e.g., already AV1, cancelled)
    Skipped { reason: String },
    /// Error occurred
    Error { message: String },
    /// Encoded but quality below threshold
    QualityWarning { vmaf: f64, threshold: f64 },
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done
                | Self::DoneWithVmaf { .. }
                | Self::DoneVmafFailed { .. }
                | Self::Skipped { .. }
                | Self::Error { .. }
                | Self::QualityWarning { .. }
        )
    }
}

/// An encoding job in the queue
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncodingJob {
    pub path: PathBuf,
    pub metadata: Option<VideoMetadata>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub track_selection: TrackSelection,
    pub status: JobStatus,
    pub output_path: Option<PathBuf>,
    pub crf: Option<u8>,
    pub source_size: Option<u64>,
    /// Source identity captured by the successful analysis.
    #[serde(default)]
    pub source_identity: Option<SourceIdentity>,
    pub output_size: Option<u64>,
    pub source_deleted: bool,
    pub source_kept_vmaf: Option<f64>,
    pub remux_only: bool,
    /// Dolby Vision handling; `None` until the user has chosen (DV sources only)
    pub dv_mode: Option<DvMode>,
    /// A ripped title staged for encoding, deleted once the encode finishes.
    /// Absent from queues written before disc ripping existed.
    #[serde(default)]
    pub temporary: bool,
}

impl EncodingJob {
    /// Create a new encoding job
    pub fn new(path: PathBuf) -> Self {
        let source_size = std::fs::metadata(&path).ok().map(|m| m.len());
        Self {
            path,
            metadata: None,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            track_selection: TrackSelection::default(),
            status: JobStatus::Pending,
            output_path: None,
            crf: None,
            source_size,
            source_identity: None,
            output_size: None,
            source_deleted: false,
            source_kept_vmaf: None,
            remux_only: false,
            dv_mode: None,
            temporary: false,
        }
    }

    /// Get the filename
    pub fn filename(&self) -> String {
        self.path.file_name().map_or_else(
            || "Unknown".to_string(),
            |n| n.to_string_lossy().to_string(),
        )
    }

    /// Get the resolution string
    pub fn resolution_string(&self) -> String {
        self.metadata
            .as_ref()
            .map_or_else(|| "Unknown".to_string(), VideoMetadata::resolution_string)
    }

    /// Get the HDR string
    pub fn hdr_string(&self) -> &str {
        self.metadata
            .as_ref()
            .map_or("Unknown", VideoMetadata::hdr_string)
    }

    /// Generate the output path based on config.
    ///
    /// A temporary source ignores `same_directory` and needs a configured
    /// output directory: its own directory is the staging directory, which is
    /// deleted as soon as the encode finishes. Without one the path is left as
    /// it stands and the job does not encode.
    pub fn generate_output_path(&mut self, output_config: &crate::config::OutputConfig) {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        let default_parent = || self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let parent = if self.temporary {
            let Some(dir) = output_config
                .output_directory
                .as_deref()
                .filter(|dir| !dir.is_empty())
            else {
                warn!(
                    "No output directory configured for the ripped file {}",
                    self.path.display()
                );
                return;
            };
            PathBuf::from(dir)
        } else if output_config.same_directory {
            default_parent()
        } else if let Some(ref dir) = output_config.output_directory {
            std::path::PathBuf::from(dir)
        } else {
            default_parent()
        };

        let suffix = if self.remux_only {
            "_remux".to_string()
        } else {
            output_config.suffix.clone()
        };

        let container = if self.remux_only {
            self.path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(&output_config.container)
                .to_string()
        } else {
            output_config.container.clone()
        };

        let mut output = parent.join(format!("{stem}{suffix}.{container}"));
        if output == self.path {
            // The output never lands on the source, whatever the suffix and
            // container add up to.
            let fallback = crate::config::OutputConfig::default().suffix;
            output = parent.join(format!("{stem}{fallback}.{container}"));
        }
        self.output_path = Some(output);
    }

    /// Bytes saved and the size change as a percentage, if both sizes are
    /// known. The percentage is negative when the encode came out larger than
    /// the source.
    pub fn size_reduction(&self) -> Option<(u64, f64)> {
        match (self.source_size, self.output_size) {
            (Some(source), Some(output)) if source > 0 => {
                let saved = source.saturating_sub(output);
                let change = i128::from(source) - i128::from(output);
                let scaled = change.unsigned_abs().saturating_mul(10_000) / u128::from(source);
                let magnitude = f64::from(u32::try_from(scaled).unwrap_or(u32::MAX)) / 100.0;
                let percent = if change < 0 { -magnitude } else { magnitude };
                Some((saved, percent))
            }
            _ => None,
        }
    }

    /// Signed byte change, positive for space saved and negative for growth.
    pub fn size_change(&self) -> Option<i128> {
        self.source_size
            .zip(self.output_size)
            .map(|(source, output)| i128::from(source) - i128::from(output))
    }
}

/// Recursively collect video files under `dir`. Symlinks are followed, and
/// directories and files are tracked by resolved path: link cycles terminate
/// and a file reachable by two routes is listed once.
pub fn collect_video_files(dir: &Path, paths: &mut Vec<PathBuf>) {
    collect_video_files_impl(dir, paths, None, None);
}

pub fn collect_video_files_cancellable_result(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<()> {
    collect_video_files_result(dir, paths, None, Some(cancel))
}

/// Recursively collect video files without following links outside `root`.
pub fn collect_video_files_within(dir: &Path, root: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(root) = root.canonicalize() else {
        return;
    };
    collect_video_files_impl(dir, paths, Some(&root), None);
}

fn collect_video_files_impl(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    root: Option<&Path>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) {
    let _ = collect_video_files_result(dir, paths, root, cancel);
}

fn collect_video_files_result(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    root: Option<&Path>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> std::io::Result<()> {
    let mut seen_dirs = HashSet::new();
    let mut seen_files = HashSet::new();
    collect_video_files_inner(dir, paths, &mut seen_dirs, &mut seen_files, root, cancel)
}

fn collect_video_files_inner(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    seen_dirs: &mut HashSet<PathBuf>,
    seen_files: &mut HashSet<PathBuf>,
    root: Option<&Path>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> std::io::Result<()> {
    if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
        return Ok(());
    }
    let real_dir = dir.canonicalize()?;
    if root.is_some_and(|root| !real_dir.starts_with(root)) {
        return Ok(());
    }
    if !seen_dirs.insert(real_dir.clone()) {
        return Ok(());
    }

    for entry in std::fs::read_dir(real_dir)? {
        if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            return Ok(());
        }
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_video_files_inner(&path, paths, seen_dirs, seen_files, root, cancel)?;
        } else if is_video_file(&path) {
            let real = path.canonicalize().unwrap_or_else(|_| path.clone());
            if root.is_none_or(|root| real.starts_with(root)) && seen_files.insert(real.clone()) {
                paths.push(if root.is_some() { real } else { path });
            }
        }
    }
    Ok(())
}

/// Make every generated output distinct from every queued source and output.
pub fn make_output_paths_unique(jobs: &mut [EncodingJob]) {
    let mut used: HashSet<PathBuf> = jobs.iter().map(|job| job.path.clone()).collect();
    used.extend(
        jobs.iter()
            .filter(|job| job.status.is_terminal())
            .filter_map(|job| job.output_path.clone()),
    );

    for job in jobs {
        if job.status.is_terminal() {
            continue;
        }
        let Some(output) = job.output_path.clone() else {
            continue;
        };
        if used.insert(output.clone()) && std::fs::symlink_metadata(&output).is_err() {
            continue;
        }

        let parent = output.parent().unwrap_or(Path::new("."));
        let stem = output.file_stem().unwrap_or_default().to_string_lossy();
        let extension = output.extension().map(|ext| ext.to_string_lossy());
        // Bounded rather than counting up indefinitely.
        let free = (2..1000)
            .map(|n| {
                let name = extension
                    .as_ref()
                    .map_or_else(|| format!("{stem}_{n}"), |ext| format!("{stem}_{n}.{ext}"));
                parent.join(name)
            })
            .find(|candidate| {
                !used.contains(candidate) && std::fs::symlink_metadata(candidate).is_err()
            });

        if let Some(candidate) = free {
            used.insert(candidate.clone());
            job.output_path = Some(candidate);
        } else {
            // Left pointing at the taken path: the encoder refuses to overwrite
            // an existing output, so the job fails rather than clobbering it.
            warn!(
                "No free output name near {} for {}",
                output.display(),
                job.path.display()
            );
        }
    }
}

/// Select audio and subtitle tracks based on configured language preferences,
/// then apply the configured audio default to whatever was selected.
pub fn auto_select_tracks(job: &mut EncodingJob, config: &TrackPresetConfig, audio: &AudioConfig) {
    // Audio tracks
    let preferred_audio: Vec<usize> = job
        .audio_tracks
        .iter()
        .filter(|t| {
            t.language.as_deref().is_some_and(|l| {
                config
                    .preferred_audio_languages
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(l))
            })
        })
        .map(|t| t.index)
        .collect();

    job.track_selection.audio_indices = if !preferred_audio.is_empty() {
        preferred_audio
    } else if config.select_all_fallback || config.preferred_audio_languages.is_empty() {
        job.audio_tracks.iter().map(|t| t.index).collect()
    } else {
        job.audio_tracks
            .first()
            .map(|t| vec![t.index])
            .unwrap_or_default()
    };

    // Subtitle tracks
    let preferred_subs: Vec<usize> = job
        .subtitle_tracks
        .iter()
        .filter(|t| {
            t.language.as_deref().is_some_and(|l| {
                config
                    .preferred_subtitle_languages
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(l))
            })
        })
        .map(|t| t.index)
        .collect();

    job.track_selection.subtitle_indices = if !preferred_subs.is_empty() {
        preferred_subs
    } else if config.select_all_fallback || config.preferred_subtitle_languages.is_empty() {
        job.subtitle_tracks.iter().map(|t| t.index).collect()
    } else {
        Vec::new()
    };

    job.track_selection.apply_audio_default(audio);
}

/// Check if a path is a video file
pub fn is_video_file(path: &Path) -> bool {
    const VIDEO_EXTENSIONS: [&str; 17] = [
        "mp4", "mkv", "avi", "mov", "webm", "m4v", "ts", "m2ts", "mts", "wmv", "flv", "mpg",
        "mpeg", "m2v", "vob", "ogv", "3gp",
    ];

    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        VIDEO_EXTENSIONS
            .iter()
            .any(|&ext| ext.eq_ignore_ascii_case(e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputConfig;

    /// No suffix and container combination puts the output on the source file.
    #[test]
    fn output_path_never_collides_with_the_source() {
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
        let config = OutputConfig {
            suffix: String::new(),
            container: "mkv".to_string(),
            same_directory: true,
            output_directory: None,
        };

        job.generate_output_path(&config);
        let output = job.output_path.clone().unwrap();
        assert_ne!(output, job.path);
        assert_eq!(output, PathBuf::from("/tmp/movie_av1.mkv"));
    }

    /// A source already named like an output still gets a distinct path.
    #[test]
    fn output_path_disambiguates_an_already_suffixed_source() {
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie_av1.mkv"));
        let config = OutputConfig {
            suffix: String::new(),
            container: "mkv".to_string(),
            same_directory: true,
            output_directory: None,
        };

        job.generate_output_path(&config);
        assert_eq!(
            job.output_path.unwrap(),
            PathBuf::from("/tmp/movie_av1_av1.mkv")
        );
    }

    /// Same-named sources in one output directory get distinct outputs, and no
    /// output collides with another queued source.
    #[test]
    fn output_paths_are_unique_across_the_queue() {
        let config = OutputConfig {
            same_directory: false,
            output_directory: Some("/out".to_string()),
            ..OutputConfig::default()
        };
        let mut jobs = vec![
            EncodingJob::new(PathBuf::from("/a/movie.mkv")),
            EncodingJob::new(PathBuf::from("/b/movie.mkv")),
            EncodingJob::new(PathBuf::from("/out/movie_av1.mkv")),
        ];
        for job in &mut jobs {
            job.generate_output_path(&config);
        }

        make_output_paths_unique(&mut jobs);

        assert_eq!(
            jobs[0].output_path,
            Some(PathBuf::from("/out/movie_av1_2.mkv"))
        );
        assert_eq!(
            jobs[1].output_path,
            Some(PathBuf::from("/out/movie_av1_3.mkv"))
        );
        assert_eq!(
            jobs[2].output_path,
            Some(PathBuf::from("/out/movie_av1_av1.mkv"))
        );
    }

    #[test]
    fn existing_outputs_are_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("av1c_output_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("movie_av1.mkv");
        std::fs::write(&existing, b"keep").unwrap();

        let mut jobs = vec![EncodingJob::new(dir.join("movie.mkv"))];
        jobs[0].output_path = Some(existing);
        make_output_paths_unique(&mut jobs);

        assert_eq!(jobs[0].output_path, Some(dir.join("movie_av1_2.mkv")));
        assert_eq!(std::fs::read(dir.join("movie_av1.mkv")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finished_output_paths_never_move_when_the_queue_changes() {
        let finished_output = PathBuf::from("/out/movie_av1.mkv");
        let mut finished = EncodingJob::new(PathBuf::from("/a/movie.mkv"));
        finished.status = JobStatus::Done;
        finished.output_path = Some(finished_output.clone());
        let mut pending = EncodingJob::new(PathBuf::from("/b/movie.mkv"));
        pending.output_path = Some(finished_output.clone());
        let mut jobs = vec![finished, pending];

        make_output_paths_unique(&mut jobs);

        assert_eq!(jobs[0].output_path, Some(finished_output));
        assert_eq!(
            jobs[1].output_path,
            Some(PathBuf::from("/out/movie_av1_2.mkv"))
        );
    }

    /// A symlink loop terminates, and a file reachable by two routes is
    /// collected once.
    #[test]
    fn recursive_collection_survives_symlink_cycles() {
        let root = std::env::temp_dir().join("av1c_scan_test");
        let _ = std::fs::remove_dir_all(&root);
        let inner = root.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("clip.mkv"), b"x").unwrap();

        #[cfg(unix)]
        {
            // inner/loop -> root, and a second route to the same file
            std::os::unix::fs::symlink(&root, inner.join("loop")).unwrap();
            std::os::unix::fs::symlink(&inner, root.join("alias")).unwrap();
        }

        let mut found = Vec::new();
        collect_video_files(&root, &mut found);
        assert_eq!(found.len(), 1, "one file, whatever route reaches it");
        assert!(found[0].ends_with("clip.mkv"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn confined_collection_never_follows_an_escaping_directory() {
        let base = std::env::temp_dir().join(format!("av1c_confined_{}", std::process::id()));
        let root = base.join("root");
        let outside = base.join("outside");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("inside.mkv"), b"x").unwrap();
        std::fs::write(outside.join("outside.mkv"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();

        let mut found = Vec::new();
        collect_video_files_within(&root, &root, &mut found);
        assert_eq!(found, vec![root.join("inside.mkv").canonicalize().unwrap()]);

        let _ = std::fs::remove_dir_all(base);
    }

    /// A queue written before disc ripping existed has no `temporary` field.
    #[test]
    fn a_job_without_a_temporary_flag_loads_as_permanent() {
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
        job.temporary = true;
        let mut value = serde_json::to_value(&job).unwrap();
        assert_eq!(value["temporary"], serde_json::json!(true));

        value.as_object_mut().unwrap().remove("temporary");
        let restored: EncodingJob = serde_json::from_value(value).unwrap();
        assert!(!restored.temporary);
    }

    /// A remux output uses the remux suffix and the source container.
    #[test]
    fn remux_output_differs_from_the_source() {
        let mut job = EncodingJob::new(PathBuf::from("/tmp/movie.mkv"));
        job.remux_only = true;
        job.generate_output_path(&OutputConfig::default());
        assert_eq!(
            job.output_path.unwrap(),
            PathBuf::from("/tmp/movie_remux.mkv")
        );
    }

    #[test]
    fn cancelled_collection_returns_no_files() {
        let dir = std::env::temp_dir().join(format!("av1c_cancel_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mkv"), b"x").unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let mut found = Vec::new();

        collect_video_files_cancellable_result(&dir, &mut found, &cancel).unwrap();

        assert!(found.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn size_reduction_reports_output_growth() {
        let mut job = EncodingJob::new(PathBuf::from("movie.mkv"));
        job.source_size = Some(100);
        job.output_size = Some(125);

        assert_eq!(job.size_reduction(), Some((0, -25.0)));
    }

    #[test]
    fn size_reduction_keeps_fractional_percentages() {
        let mut job = EncodingJob::new(PathBuf::from("movie.mkv"));
        job.source_size = Some(10_000);
        job.output_size = Some(9_950);

        assert_eq!(job.size_reduction(), Some((50, 0.5)));
    }
}
