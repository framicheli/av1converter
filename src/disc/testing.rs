//! The fake `makemkvcon` the disc tests run against, and the robot-output
//! fixtures they parse.
//!
//! One script covers every mode the tests need, picked with [`Fake`]. Tests
//! reach it through `disc.makemkvcon_path` or by passing the path directly,
//! which is the route a real install takes.
//!
//! Unix only: the script is `/bin/sh`. Every test that uses it is gated
//! `#[cfg(unix)]`, so the suite still runs — with the process-level tests
//! skipped — on Windows.

use std::path::{Path, PathBuf};

/// What the fake does when asked to rip.
pub enum Fake {
    /// A DVD scan: four episodes plus a short extra.
    Dvd,
    /// A Blu-ray scan: one feature, one commentary angle.
    Bluray,
    /// Rips instantly, writing an empty file.
    Rip,
    /// Rips over about a second, in four progress steps.
    SlowRip,
    /// Reports progress, then fails partway with a read error.
    RipFails,
    /// Reports progress and then hangs until it is killed.
    RipHangs,
    /// A Blu-ray whose key has expired.
    ExpiredKey,
    /// A drive the daemon user cannot open.
    PermissionDenied,
    /// Answers `file:` and `iso:` sources, recording the argv it was called
    /// with. Its drive listing fails, as it does on a machine with no drive.
    FolderScan,
}

impl Fake {
    /// The `case` body that answers a scan, and the one that answers a rip.
    fn behaviour(&self) -> (&'static str, &'static str) {
        let rip_ok = "  echo 'PRGC:5057,0,\"Saving to MKV file\"'\n  \
             echo 'TINFO:0,27,0,\"title_t00.mkv\"'\n  \
             echo 'PRGV:0,65536,65536'\n  \
             : > \"$dest/title_t00.mkv\"\n  exit 0";
        match self {
            Fake::Dvd => (DVD_SCAN, rip_ok),
            Fake::Bluray | Fake::Rip | Fake::FolderScan => (BLURAY_SCAN, rip_ok),
            Fake::SlowRip => (
                BLURAY_SCAN,
                "  for step in 16384 32768 49152 65536; do\n    \
                   echo \"PRGV:0,$step,65536\"\n    sleep 0.2\n  done\n  \
                 echo 'TINFO:0,27,0,\"title_t00.mkv\"'\n  \
                 : > \"$dest/title_t00.mkv\"\n  exit 0",
            ),
            Fake::RipFails => (
                BLURAY_SCAN,
                "  echo 'PRGV:0,16384,65536'\n  \
                 : > \"$dest/title_t00.mkv\"\n  \
                 echo 'MSG:5003,16,2,\"Scsi error - MEDIUM ERROR\",\"x\"'\n  exit 1",
            ),
            Fake::RipHangs => (BLURAY_SCAN, "  echo 'PRGV:0,1,65536'\n  exec sleep 300"),
            Fake::ExpiredKey => (EXPIRED_KEY, "  exit 1"),
            Fake::PermissionDenied => (PERMISSION_DENIED, "  exit 1"),
        }
    }
}

/// Write the fake and return its path. The directory it lives in doubles as a
/// rip destination.
pub fn fake_makemkvcon(dir: &Path, mode: &Fake) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let (scan, rip) = mode.behaviour();
    let drives = match mode {
        Fake::PermissionDenied => PERMISSION_DENIED,
        Fake::FolderScan => NO_DRIVE,
        _ => DRIVE_LIST,
    };
    // A folder source is answered like a scan, with the argv kept for the test
    // to read back.
    let folder = match mode {
        Fake::FolderScan => format!(
            "*file:*|*iso:*)\n  echo \"$*\" > \"$(dirname \"$0\")/argv\"\n{scan}\n  exit 0 ;;\n"
        ),
        _ => String::new(),
    };
    // Every subcommand is matched space-delimited, so a destination or source
    // path containing "info" or "mkv" cannot answer with the wrong branch.
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n*disc:9999*)\n{drives}\n  exit 1 ;;\n\
         *\\ mkv\\ *)\n  for dest; do :; done\n{rip} ;;\n\
         {folder}\
         *\\ info\\ *)\n{scan}\n  exit 0 ;;\n\
         *)\n  echo \"fake makemkvcon: unmatched argv: $*\" >&2\n  exit 1 ;;\nesac\n"
    );

    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("makemkvcon");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A scratch directory of this test's own, emptied on the way in.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("av1c_disc_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const NO_DRIVE: &str = "  echo 'MSG:5010,0,0,\"No optical drive found\",\"x\"'";

const DRIVE_LIST: &str = "  echo 'DRV:0,2,999,12,\"HL-DT-ST BD-RE WH16NS60\",\"THE_DISC\",\"/dev/sr0\"'\n  \
     echo 'DRV:1,256,999,0,\"\",\"\",\"\"'";

/// A four-episode DVD with a short extra, as `makemkvcon -r info` reports one.
///
/// Written from `MakeMKV`'s documented robot format rather than captured from
/// a drive: no optical hardware was available. Shapes and attribute ids match
/// the format; the values are representative.
pub const DVD_SCAN: &str = r#"  echo 'MSG:1005,0,1,"MakeMKV v1.17.9 started","x"'
  echo 'DRV:0,2,999,12,"HL-DT-ST BD-RE WH16NS60","SEASON_1_DISC_2","/dev/sr0"'
  echo 'MSG:3007,0,0,"Using direct disc access mode","x"'
  echo 'CINFO:1,6206,"DVD disc"'
  echo 'CINFO:2,0,"SEASON_1_DISC_2"'
  echo 'CINFO:32,0,"SEASON_1_DISC_2"'
  echo 'TCOUNT:5'
  echo 'TINFO:0,2,0,"Episode 5, The Return"'
  echo 'TINFO:0,8,0,"6"'
  echo 'TINFO:0,9,0,"44:12"'
  echo 'TINFO:0,10,0,"1.6 GB"'
  echo 'TINFO:0,11,0,"1717986918"'
  echo 'TINFO:0,27,0,"title_t00.mkv"'
  echo 'SINFO:0,0,1,6201,"Video"'
  echo 'SINFO:0,0,6,0,"MPEG-2"'
  echo 'SINFO:0,0,19,0,"720x576"'
  echo 'SINFO:0,1,1,6202,"Audio"'
  echo 'SINFO:0,1,4,0,"English"'
  echo 'SINFO:0,1,6,0,"DD"'
  echo 'SINFO:0,1,40,0,"5.1"'
  echo 'SINFO:0,2,1,6203,"Subtitles"'
  echo 'SINFO:0,2,4,0,"English"'
  echo 'TINFO:1,2,0,"Episode 6"'
  echo 'TINFO:1,8,0,"6"'
  echo 'TINFO:1,9,0,"43:58"'
  echo 'TINFO:1,11,0,"1701208883"'
  echo 'SINFO:1,0,1,6201,"Video"'
  echo 'SINFO:1,0,6,0,"MPEG-2"'
  echo 'TINFO:2,2,0,"Episode 7"'
  echo 'TINFO:2,9,0,"44:01"'
  echo 'TINFO:2,11,0,"1705000000"'
  echo 'TINFO:3,2,0,"Episode 8"'
  echo 'TINFO:3,9,0,"45:30"'
  echo 'TINFO:3,11,0,"1750000000"'
  echo 'TINFO:4,2,0,"Behind the Scenes"'
  echo 'TINFO:4,9,0,"6:20"'
  echo 'TINFO:4,11,0,"240000000"'
  echo 'MSG:5010,0,0,"Operation successfully completed","x"'"#;

/// A Blu-ray feature with a commentary angle, quoted title included.
///
/// Constructed the same way as [`DVD_SCAN`], and for the same reason.
pub const BLURAY_SCAN: &str = r#"  echo 'MSG:3007,0,0,"Using direct disc access mode","x"'
  echo 'CINFO:1,6209,"Blu-ray disc"'
  echo 'CINFO:2,0,"THE_DISC"'
  echo 'TCOUNT:2'
  echo 'TINFO:0,2,0,"Blade Runner, The \"Final\" Cut"'
  echo 'TINFO:0,8,0,"32"'
  echo 'TINFO:0,9,0,"1:57:31"'
  echo 'TINFO:0,10,0,"27.6 GB"'
  echo 'TINFO:0,11,0,"29715223808"'
  echo 'TINFO:0,27,0,"title_t00.mkv"'
  echo 'SINFO:0,0,1,6201,"Video"'
  echo 'SINFO:0,0,6,0,"MPEG-4 AVC"'
  echo 'SINFO:0,0,19,0,"1920x1080"'
  echo 'SINFO:0,1,1,6202,"Audio"'
  echo 'SINFO:0,1,4,0,"English"'
  echo 'SINFO:0,1,6,0,"DTS-HD MA"'
  echo 'SINFO:0,1,40,0,"5.1"'
  echo 'SINFO:0,2,1,6202,"Audio"'
  echo 'SINFO:0,2,4,0,"Italiano"'
  echo 'SINFO:0,2,6,0,"DD"'
  echo 'SINFO:0,2,40,0,"2.0"'
  echo 'TINFO:1,2,0,"Commentary"'
  echo 'TINFO:1,9,0,"1:57:31"'
  echo 'TINFO:1,11,0,"8000000000"'"#;

const EXPIRED_KEY: &str = "  echo 'MSG:5021,0,0,\"The beta registration key has expired\",\"x\"'";

const PERMISSION_DENIED: &str =
    "  echo 'MSG:3006,0,0,\"Error \\\"Permission denied\\\" opening /dev/sr0\",\"x\"'";
