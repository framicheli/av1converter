# Disc ripping — manual checklist

Automated tests run against a fake `makemkvcon` (`src/disc/testing.rs`). They
cannot cover an optical drive, a real disc, or MakeMKV itself. Run this list
against real hardware before a release that touches `src/disc/`.

Record the date, the MakeMKV version, and the platform with the results.

| # | Case | What to look for |
|---|---|---|
| 1 | DVD movie | One long title selected, ripped, encoded; output named after the disc label |
| 2 | DVD episodic disc | Every episode listed, several selectable, extras visible and short |
| 3 | Blu-ray movie | Title list is not swamped by playlist decoys; size estimate is plausible |
| 4 | Blu-ray episodic disc | Episodes ripped one after another, encoding overlapping the next rip |
| 5 | Encrypted disc | Decryption succeeds with a valid key; no silent partial rip |
| 6 | Damaged disc | Read failure reported in the user's language, not a raw MakeMKV line alone |
| 7 | Disc ejected mid-rip | Rip fails, staging directory deleted, queue shows the failure |
| 8 | Cancel during rip | `makemkvcon` dies promptly, partial file and directory removed |
| 9 | Expired Blu-ray beta key | The key message appears, saying it is MakeMKV's key and not this tool |
| 10 | Daemon without drive permissions | Permission message names the group problem, not "ripping failed" |
| 11 | Staging disk filling up mid-rip | Failure is reported and the partial file is cleaned up |
| 12 | Kill the daemon mid-rip | No `makemkvcon` survives; the next start sweeps the staging directory |

Both front ends are worth one pass each: the TUI (`Rip DVD / Blu-ray` on the
home menu) and the web UI (`+ Disc`).

## Results

| Date | Version | Platform | Cases run | Notes |
|---|---|---|---|---|
| 2026-08-13 | — | macOS 15 (arm64) | none | MakeMKV is not installed on the development machine and it has no optical drive. Every case above is unrun. The fake-binary suite passed. |
