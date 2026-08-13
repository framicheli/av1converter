# Changelog

## [Unreleased]

### Added

- **Disc ripping.** Import titles straight from a DVD or Blu-ray through MakeMKV, from the TUI (`Rip DVD / Blu-ray` on the home menu) or the web UI (`+ Disc`). Titles are extracted to a staging directory and then analyzed, track-configured and encoded exactly like a file opened by hand.
  - Ripping and encoding overlap: the next title reads from the disc while the previous one encodes, so peak disk use stays at one rip plus one encode input.
  - A rip appears in the queue as a job of its own, with progress and cancellation in the same place as everything else.
  - Staging files are deleted once their encode succeeds, kept when it fails or comes in under the VMAF threshold, and swept at startup when a rip was cut short.
  - Output files are named after the disc label (`<label>_t<NN>_av1.mkv`) rather than MakeMKV's `title_t00.mkv`.
- `--scan-discs`, a diagnostic that prints the drives and the titles MakeMKV reports.
- `[disc]` configuration block: `makemkvcon_path` (unset resolves through `PATH` and the platform's install location) and `staging_directory` (unset stages under the system temp directory). Both are config/TUI-only, like `browse_root` and `auth_token` — the browser never names a binary the daemon executes.
- Four token-guarded API endpoints — `GET /api/discs`, `POST /api/discs/{scan,rip,cancel}` — accepting only drive and title ids the server itself reported. Disc state rides in `/api/status`, so the page still has one poll loop.
- Disc failures are reported in the user's language and told apart from one another: MakeMKV missing, no drive, empty drive, expired Blu-ray key, unreadable disc, permission denied, insufficient space, a swapped disc, and cancellation.

### Fixed

- Cancelling a `makemkvcon` run could block until its child processes exited; the output reader is no longer waited on after a cancellation.
- A job's analysis result no longer overwrites a status the job had already reached, so a file that failed keeps the reason it failed.

### Known limitations

- Dolby Vision profile 7 UHD discs are extracted by MakeMKV with the enhancement layer as a separate MKV track, which FFmpeg will not recombine. Those discs encode from the HDR10 base layer and the EL/RPU track shows up as a stray stream.
- The disc test suite runs against a fake `makemkvcon`; the manual checklist in `docs/disc-manual-checklist.md` has not been run against real hardware.

## [3.0.0]

Initial public release: interactive TUI for batch-converting video to AV1 with FFmpeg — hardware encoder auto-detection (NVENC/QSV/AMF), Dolby Vision passthrough and profile 5 tone-mapping, VMAF quality verification, per-track audio/subtitle selection with Opus transcoding, daemon mode with a web UI, and a six-language interface.
