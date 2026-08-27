# Changelog

## [Unreleased]

### Added

- `--start` and `--restart` daemon lifecycle commands. The former replaces `--daemon` in the documented interface, while `--daemon` remains a compatibility alias; `--start-foreground` likewise aliases the old foreground spelling.
- **Disc ripping.** Import titles straight from a DVD or Blu-ray through MakeMKV, from the TUI (`Rip DVD / Blu-ray` on the home menu) or the web UI (`+ Disc`). Titles are extracted to a staging directory and then analyzed, track-configured and encoded exactly like a file opened by hand.
  - Ripping and encoding overlap: the next title reads from the disc while the previous one encodes, so peak disk use stays at one rip plus one encode input.
  - A rip appears in the queue as a job of its own, with progress and cancellation in the same place as everything else.
  - Staging files are deleted once their encode succeeds, kept when it fails or comes in under the VMAF threshold, and swept at startup when a rip was cut short.
  - Output files are named after the disc label (`<label>_t<NN>_av1.mkv`) rather than MakeMKV's `title_t00.mkv`.
- `--scan-discs`, a diagnostic that prints the drives and the titles MakeMKV reports.
- `--purge`, which deletes configuration and daemon state after confirmation. Refuses while the daemon is running.
- **Run at Startup.** A Settings toggle (and `--install-service` / `--uninstall-service`) installs a systemd user unit on Linux or a launchd agent on macOS so the web UI comes up at login. Not stored in `config.toml`; `--status` reports whether it is installed. `--purge` removes the unit too.
- `[disc]` configuration block: `makemkvcon_path` (unset resolves through `PATH` and the platform's install location) and `staging_directory` (unset stages under the system temp directory).
- Complete settings parity between the TUI and web UI, including every per-tier encoder value, film-grain strength, track fallback, daemon/service controls, and disc paths. Host-sensitive values are writable from loopback web sessions and read-only to remote sessions.
- Four token-guarded API endpoints — `GET /api/discs`, `POST /api/discs/{scan,rip,cancel}` — accepting only drive and title ids the server itself reported. Disc state rides in `/api/status`, so the page still has one poll loop.
- Disc failures are reported in the user's language and told apart from one another: MakeMKV missing, no drive, empty drive, expired Blu-ray key, unreadable disc, permission denied, insufficient space, a swapped disc, and cancellation.

### Fixed

- Cancelling a `makemkvcon` run could block until its child processes exited; the output reader is no longer waited on after a cancellation.
- A job's analysis result no longer overwrites a status the job had already reached, so a file that failed keeps the reason it failed.
- The web UI's Cancel button now stops an in-progress disc rip, not only an encode. `+ Disc` is disabled while a rip is running.
- Daemon shutdown now kills leftover ffmpeg/ffprobe/makemkvcon children after the grace period and waits for the encode and rip worker threads to finish.
- A TUI disc rip appends to the queue instead of wiping it, and rip events follow those jobs rather than raw queue positions.
- Turning off Run at Startup from the TUI uninstalls the login unit without stopping a daemon that is already running, matching the web UI.
- Drive listings can be cancelled; `disc.active` is the shared flag those listings set, so the dashboard can stop them.
- Late MakeMKV progress for a title that has already extracted no longer resets that job to ripping.
- A successful queue save keeps the previous `queue.json` as `queue.json.bak`; a corrupt file reloads from that backup.
- Disc identity is drive id, drive name, disc label, and title id plus title name, so a same-label swap is caught before extraction.
- Web status pill prefers verifying over encoding; the track modal no longer opens over unsaved Settings; cancelled jobs appear in the batch summary; cancel uses an in-page confirm; a skipped poll tick is retried.
- The TUI home title and the disc Discovering screen follow the selected language. `--start` mints an auth token only after taking the PID lock.
- After a forced shutdown, leftover encode/rip messages are applied before `queue.json` is written, so a killed encode is not restarted as Ready. HTTP mutations are refused once shutdown starts, and in-flight requests finish before worker handles are taken.
- Cancel encoding no longer silently cancels a concurrent disc rip. The web disc browser can pick an ISO. Analysis can be cancelled from the dashboard. Removing or clearing a ripped job deletes its staging file.
- Recursive folder add respects shutdown. Unix child kill covers the process group. TUI quit uses the same grace-then-kill path. Finish lists VMAF-check failures, Esc returns to the queue, and Enter confirms before clearing.

### Known limitations

- Dolby Vision profile 7 UHD discs are extracted by MakeMKV with the enhancement layer as a separate MKV track, which FFmpeg will not recombine. Those discs encode from the HDR10 base layer and the EL/RPU track shows up as a stray stream.
- The disc test suite runs against a fake `makemkvcon`; the manual checklist in `docs/disc-manual-checklist.md` has not been run against real hardware.

## [3.0.0]

Initial public release: interactive TUI for batch-converting video to AV1 with FFmpeg — hardware encoder auto-detection (NVENC/QSV/AMF), Dolby Vision passthrough and profile 5 tone-mapping, VMAF quality verification, per-track audio/subtitle selection with Opus transcoding, daemon mode with a web UI, and a six-language interface.
