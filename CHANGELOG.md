# Changelog

## [3.0.0] - 2026-09-19

Changes since 2.6.1. This release adds disc ripping through MakeMKV, login autostart, and matching settings in the TUI and the web UI. It also hardens the daemon and the encode pipeline. Read **Breaking changes** before upgrading a daemon that is reachable from the network.

### Breaking changes

- A daemon bound outside loopback no longer starts over plain HTTP unless `allow_insecure_lan = true` is set under `[daemon]`. It also needs a non-empty `browse_root`. Both are checked when settings are saved and when the daemon starts; 2.6.1 only printed a warning.
- The web UI reads the access token from `#token=…` instead of `?token=…`, so old `?token=` links no longer log in. `av1converter --status` prints the new link.
- An `auth_token` shorter than 32 characters is replaced with a generated one when the daemon starts. A replacement entered in either settings page must have at least 32 characters, all printable ASCII without spaces.
- `output.container` accepts only `mkv`, `mp4` and `webm`; any other value loads as `mkv`.
- With `quality_preset` set to `low`, `medium` or `high`, the `[presets.*]` tables are rewritten from the preset whenever the configuration is loaded or saved. Set `quality_preset = "custom"` to keep hand-edited per-tier values.
- `disc.makemkvcon_path` must name a file called `makemkvcon` or `makemkvcon64` (with or without `.exe`). A wrapper script, such as one for a Flatpak install, has to be named `makemkvcon`.
- The command line takes a single option. Extra arguments print the usage and exit with status 2.

### Changed

- `--start` and `--start-foreground` replace `--daemon` and `--daemon-foreground`, which remain as aliases.
- Stopping or restarting the daemon keeps the running and queued jobs for the next start instead of marking them cancelled; the file that was encoding starts over, and files being analysed are analysed again.

### Added

- `--restart`, which stops a running daemon cleanly and starts it again.
- **Disc ripping.** Import titles straight from a DVD or Blu-ray through MakeMKV, from the TUI (`Rip DVD / Blu-ray` on the home menu) or the web UI (`+ Disc`). Titles are extracted to a staging directory and then analyzed, track-configured and encoded exactly like a file opened by hand.
  - Ripping and encoding overlap: the next title reads from the disc while the previous one encodes, so peak disk use stays at one rip plus one encode input.
  - A rip appears in the queue as a job of its own, with progress and cancellation in the same place as everything else.
  - Staging files are deleted once their encode succeeds, kept when it fails, comes in under the VMAF threshold or cannot be VMAF-checked, and swept at startup when a rip was cut short.
  - Output files are named after the disc label (`<label>_t<NN>_av1.mkv`) rather than MakeMKV's `title_t00.mkv`.
- `--scan-discs`, a diagnostic that prints the drives and the titles MakeMKV reports.
- `--purge`, which deletes configuration and daemon state after confirmation. Refuses while the daemon is running.
- **Run at Startup.** A Settings toggle (and `--install-service` / `--uninstall-service`) installs a systemd user unit on Linux or a launchd agent on macOS so the web UI comes up at login. Not stored in `config.toml`; `--status` reports whether it is installed. `--purge` removes the unit too.
- `[disc]` configuration block: `makemkvcon_path` (unset resolves through `PATH` and the platform's install location) and `staging_directory` (unset stages under the system temp directory).
- Complete settings parity between the TUI and web UI, including every per-tier encoder value, film-grain strength, track fallback, daemon/service controls, and disc paths. Host-sensitive values are writable from loopback web sessions and read-only to remote sessions.
- Four token-guarded API endpoints — `POST /api/discs/{list,scan,rip,cancel}` — accepting only drive and title ids the server itself reported. Disc state rides in `/api/status`, so the page still has one poll loop.
- Disc failures are reported in the user's language and told apart from one another: MakeMKV missing, no drive, empty drive, expired Blu-ray key, unreadable disc, permission denied, insufficient space, a swapped disc, and cancellation.
- `daemon.behind_proxy` (TUI and web Settings): treats every web request as remote, for a reverse proxy on the daemon host that a loopback browser cannot be told apart from. Off by default, so a browser on the daemon host keeps full settings access.
- TUI: daemon rows that need a restart say so, as in the web UI.
- Web UI: the autostart setting explains what it does, as in the TUI.
- TUI: `a` on the disc title list selects every title, or clears them.
- TUI: with a single drive, the disc flow scans it straight away, as the web UI does.
- TUI: the queue title shows overall progress and space saved, and the summary counts cancelled jobs apart from skipped ones.
- TUI: encoding starts as soon as the first job's tracks are confirmed, and `t` reopens the tracks of any job not being encoded, as in the web UI.
- TUI: `A` in track selection applies the choices to every file still waiting, matching tracks by order, as the web UI's "Apply to remaining files".
- TUI: files, folders and discs can be added while an encode, analysis or rip runs (`a` in the queue, `Esc` on Home returns to it); new files join the queue instead of replacing it, and files already queued are skipped. Abandoning track selection drops only the files not yet finished.
- TUI: `x` removes a waiting or finished job from the queue; a ripped title asks first and its staging files are deleted.
- TUI: `C` clears finished jobs from the queue while nothing runs, keeping the space-saved total; ripped titles ask first.
- Web UI: the queue page shows the encoder and VMAF status, and warns when this FFmpeg build lacks the encoder, libvmaf or libopus, as the TUI does.
- Web UI: queue rows show the CRF, the VMAF rating and threshold, "source kept" and the output file name (on hover), the track dialog names the output file, and the overall panel shows elapsed time, as the TUI does.
- Web UI: the Dolby Vision choice explains each option, marks the recommended one and warns about Profile 5, as the TUI dialog does.
- Web UI: an "All to Opus" toggle converts every selected audio track at once, or back to copy, like `O` in the TUI.
- Web UI: the file browser stays open after adding a file, so several files can be added in one go.

### Fixed

- TUI cancel confirm (Esc → Yes) actually cancels encoding, analysis, and disc work instead of only closing the dialog.
- Encode/probe/VMAF cancel kills the whole process group; shutdown `kill_all` skips reused PIDs and on Windows no longer requires the child image to match this binary.
- Output publish never clobbers an existing file (exclusive create instead of checked rename).
- Cancelling after a successful encode or during VMAF removes the finished output so a retry is not blocked.
- VMAF auto-delete and quality warnings require both mean and minimum sampled-frame scores; UIs and logs report the min as well as the mean.
- Stopping the daemon during a VMAF check no longer marks the job done with no output file; it is encoded again on the next start, and a ripped title stays staged.
- Cancelling an encode or an analysis in the TUI keeps queued ripped titles, as the web UI does, instead of deleting them.
- The staging sweep removes only the folders AV1Converter created for a rip. A folder of your own in the staging directory whose name starts with `rip-` is no longer deleted.
- Applying track choices to remaining files copies the Dolby Vision choice only to files of the same Dolby Vision profile. A profile 5 file no longer inherits "keep Dolby Vision" from a profile 8 file, and a profile 8 file no longer loses its Dolby Vision because a profile 5 file was set to HDR10.
- A job set to keep Dolby Vision that runs on a hardware encoder, which converts it to HDR10, now keeps its source instead of deleting it after VMAF passes. The log says why.
- Sources with streams the output does not carry, such as a second video stream, a data stream (timecode, GPS) or a Dolby Vision profile 7 enhancement layer, are no longer deleted after a passing VMAF check. Sources whose streams cannot be probed are kept too.
- Sources that are not 4:2:0 at 10 bits or fewer (4:2:2, 4:4:4, RGB, alpha or 12-bit video), or whose pixel format cannot be read, are no longer deleted after a passing VMAF check, since the 4:2:0 10-bit encode reduces them in ways VMAF does not measure.
- A queue file whose selected subtitles no longer match the source's tracks keeps the source, and a subtitle with no known output codec is left out instead of being written with the container's default.
- The TUI encodes and analyses with the saved configuration. Unsaved edits on the Configuration screen, such as a lower VMAF threshold or turning on source deletion, no longer apply to the next job.
- Portrait 4K uses the 4K VMAF models (long side ≥ 3840).
- Dolby Vision without a readable `dv_profile` fails analysis instead of encoding IPT as bare PQ.
- `film_grain` is SVT-AV1 only in settings; the web Settings tab no longer breaks when a hardware encoder is selected.
- Web i18n refresh no longer clobbers live status text or clears the file-browser pick callback.
- Queue add reports true duplicates separately from other skips; concurrent add of the same inode is deduped.
- Empty auth tokens never open the API.
- Cancelling disc listing respects shutdown; encode/disc workers join within the shutdown grace.
- Preferred languages match BCP-47 region tags (`en-US` ↔ `eng`).
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
- AMF encodes no longer come out near-lossless: the 0–51 quality value is scaled onto AMF's 0–255 quantizer.
- The output container is limited to `mkv`, `mp4` and `webm`. Streams a container cannot hold are left out or converted instead of failing the encode: bitmap subtitles are dropped from WebM (and from MP4, except DVD subtitles), WebM audio that is not Opus or Vorbis becomes Opus, and TrueHD copies into MP4 work.
- MKV output keeps attachments such as embedded fonts and cover art.
- The output container is chosen from `mkv`, `mp4` and `webm` in both settings screens, and the track screens show which audio tracks the container converts to Opus (with the bitrate) and which selected subtitles it leaves out.
- VMAF verification works when the temporary path contains `:`, as every Windows path does.
- Dolby Vision profile 8.2 sources (SDR base layer) are tagged BT.709 instead of PQ/BT.2020.
- QSV no longer passes the `-look_ahead` option, which `av1_qsv` does not have.
- A deselected audio track that runs past the video no longer makes a finished encode count as stopped short.
- An encode whose video stops short is caught even when an audio track still runs the full length, and a shortfall of more than 0.5% of the source (at least two seconds) now counts, down from 2%.
- An analysis failure on a path containing "Cancelled" is reported as an error, not a cancellation.
- Empty or relative `XDG_*`, `HOME`, `APPDATA` and `LOCALAPPDATA` values are ignored instead of placing configuration in the working directory. A non-UTF-8 command-line argument prints usage instead of panicking.
- The private temporary directory is recreated when the system cleans it up under a long-running daemon.
- The launchd agent receives `PATH`, so an FFmpeg installed through Homebrew is found at login. Turning autostart off also disables the launchd job.
- The systemd unit stops with `KillMode=mixed` and a stop timeout that covers the daemon's own grace, so a stop cancels the running job cleanly; `%` and `$` in the binary path are escaped.
- `--start` reports success only once the daemon is listening, and a failed bind as a failure. Shutdown cancels disc and analysis work before waiting for HTTP requests and no longer waits indefinitely for a stalled client. A running probe is reached by cancellation. A PID lock can no longer be taken on an already-deleted PID file.
- Starting the daemon no longer deletes staged rips that a running TUI is waiting to encode, quitting the TUI removes its staged rips, leftover staging directories are also swept when a disc run ends, and cancelling a rip kills makemkvcon's whole process group.
- A ripped title with no output directory is reported as an error instead of waiting in Ready forever.
- On Windows, staging directories and partial encodes owned by a running instance are recognised as in use instead of being treated as leftovers.
- `--install-service` reports a daemon that fails to start (a port already in use, for example) as a failure and removes the unit or agent, instead of claiming it will start at login. A daemon that is still starting after 30 seconds stays installed, with a note to check `--status`.
- The launchd agent runs as a standard process rather than a background one, so macOS no longer throttles its disk access; `Nice` still lowers its CPU priority.
- `--purge` removes the login service only after the confirmation.
- Web API: the output directory is confined to `browse_root` even when outputs go next to their sources; a saved directory that has since disappeared no longer blocks saving other settings and is reported as a warning. Disc scans and rips no longer hold the daemon's state lock during filesystem checks, and are refused once shutdown starts. Saving tracks for a Dolby Vision job after switching to a hardware encoder works. Concurrent settings, autostart and queue-add requests no longer overwrite each other or bypass the browse root. The cancelled count survives clearing finished jobs. Folder mode skips directories named like videos, omitted track lists keep their values, requests through a reverse proxy with forwarding headers are treated as remote, and a padded browse root is trimmed. Old disc titles are cleared when drives are listed.
- Web UI: removing a ripped job, or clearing finished jobs that include one, asks first. Settings are re-read when the tab opens. Closing a tracks dialog stops further automatic prompts until new files arrive. The disc dialog no longer flashes "No drive found" or old titles. Keyboard focus survives Move up and disc dialog redraws. The file browser cannot select a folder it did not load. Adding `#token=` to an open page logs in. Interface strings are fetched again after a failed load, durations are translated, "Queue is empty" waits for the queue to load, refreshes after actions show the new state, and cancel re-checks what is running after the confirmation. The breadcrumb and track rows fit phone widths.
- TUI: a rip that fails while being cancelled no longer leaves the queue stuck on "Cancelling". A finished job no longer throws you out of track configuration. Cancelling analysis keeps already analysed jobs and does nothing once analysis has finished, and it reaches every analysis batch. Ctrl and Alt shortcuts no longer type letters into text fields, and `Shift+Tab` moves focus backwards. Long notices and the configuration footer wrap instead of being cut off, text uses the terminal's default colour, `Space` selects the open folder in folder mode, quitting with unsaved settings says so, a scan finishing during a cancel returns to the drive list, and the remaining English status messages are translated.
- Tests use a private configuration path per test thread instead of the user's configuration or a process-wide environment variable.
- Small terminals keep the home menu and its notices visible, disc titles and other normal text use the terminal's default colour, the Performance heading appears once, titles without chapters omit the count (TUI, web and `--scan-discs`), ripped titles no longer claim their source was deleted, quitting warns that ripped titles still in the queue will be deleted, long configuration values end with an ellipsis, and the Opus target stays visible in narrow track panels.
- The web page no longer scrolls sideways on phones, and queue sizes stay on one line.
- The cancel prompt says that jobs waiting to encode are cancelled with the current encode.
- The Nix flake is pinned with `flake.lock`, and rebuilding it produces identical output.
- Cover art that comes before the video in an MP4/MOV is no longer analysed, encoded and VMAF-checked in place of the film, which could delete the source after a one-frame encode.
- The source is kept when a selected subtitle track is converted or left out, or when cover art or other attachments are not carried into the output.
- The encoded output is flushed to disk before the source is deleted; if the flush fails, the source is kept.
- Removing or clearing a ripped job only deletes a staging directory under the staging root, never a directory named after a disc title elsewhere.
- Restarting the daemon no longer marks jobs as outside `browse_root` because their source was deleted or the share is not mounted yet. Finished jobs keep their history, and a staged rip is no longer swept while its job still exists.
- Ripping no longer fails with "disc changed" when the drive list was read before the disc finished loading; staged files carry the disc label.
- First-run encoder detection encodes one test frame with each hardware AV1 encoder instead of matching GPU names, so Turing Quadros, AV1-decode-only GPUs and AMD cards are no longer given an encoder they cannot use; Windows no longer calls the deprecated wmic.
- A config section with only some keys keeps the defaults for the rest instead of discarding the whole file; a config.toml that cannot be read is reported in the TUI status line.
- A `[presets.<tier>]` table with only some values keeps that tier's defaults for the others instead of discarding the whole configuration.
- Web settings under a browse root refuse an output directory that is outside it or missing with the same message, so a remote session cannot use the check to learn which directories exist on the host.
- `POST /api/settings` with only some keys changes just those keys instead of resetting the rest to defaults. `POST /api/job/tracks` refuses a track list, `remux_only` or `apply_to_remaining` of the wrong type instead of ignoring it.
- Web settings refused because of a job queued during the save report an error when the previous settings cannot be written back, instead of claiming nothing changed while `config.toml` holds the refused values.
- TUI: once a file starts encoding, its open track configuration says so and ignores track, remux and Dolby Vision changes, instead of applying them to the running encode.
- A file under the VMAF threshold is marked "Source kept" only when source deletion is on. The TUI names the score that failed (the mean, or the minimum when only it is low) against the threshold the file was checked with, and the web shows the same on the tag's tooltip and no longer rounds the threshold in the badge.
- TUI: notices such as "Tracks applied to …" and "Removed … finished" show on the queue screen when they happen, instead of later on Home.
- Running work can be cancelled one kind at a time: on the TUI queue `A` cancels the analysis and `E` the encode while a rip or analysis also runs, and the web shows a separate Cancel analysis button during an encode. Cancelling an encode in the TUI no longer also stops a running rip.
- TUI: `b` in Settings picks the output directory, browse root or staging directory in the file explorer, as the web's Browse button does.
- Web: the track dialog's "Opus unavailable" warning also appears when the output container forces Opus, and no longer appears for tracks that are already Opus and are copied, as in the TUI.
- Web: settings can be saved when `config.toml` holds a VMAF threshold with more than one decimal, such as 93.25.
- Web: an open track dialog turns read-only with the "already encoding" note once its file starts encoding, instead of offering a Save that fails.
- Web settings cannot clear browse_root while the running daemon listens outside loopback, even when the saved bind address is loopback.
- A web request from `::ffff:127.0.0.1` counts as loopback.
- The output directory disc rips need can be set in the web UI and TUI while "same directory" is on; the add-to-queue toast reports skipped and already-queued files separately and is translated.
- Web UI: closing a track dialog without saving no longer opens the next job's dialog a second later, and an automatic track dialog no longer opens over another dialog.
- An infinite `DURATION` tag no longer makes the saved queue unreadable.
- A recursive add skips an unreadable subfolder instead of stopping at it.
- The access token must be printable ASCII without spaces, so the web UI can always send it.
- A daemon starting while `--status`, `--start` or service install checks on it no longer exits at once.
- `--stop`, systemd and launchd wait long enough for the daemon's final queue save.
- Windows: an empty PID file left by a crash no longer blocks every start, and a failed `tasklist` no longer removes a running daemon's PID file.
- Delete-on-success keeps a symlinked source rather than removing the link and reporting it deleted.
- Adding files reads their identities without holding the daemon lock.
- A machine with no drive, or a disc with no usable titles, is reported as "no drive" or "drive empty" instead of quoting MakeMKV's startup banner or "Operation successfully completed".
- A long rip that fails is reported with MakeMKV's closing messages, not with the first 200 it printed.
- The default staging folder is private to the user (`av1converter-staging-<uid>`, mode 0700 on Unix), so another user on the machine can no longer claim it or tamper with a rip.
- A rip cut short by a crash is cleaned up when the TUI or the daemon next starts, instead of lingering for 30 minutes or, after a TUI crash, until the daemon runs.
- Rips from a `BDMV` or `VIDEO_TS` folder are named after the folder above it, and rips from an image drop the `.iso` extension from the name.
- When a rip run stops on an error, the titles it never reached are skipped with that error instead of being shown and counted as cancelled.
- A rip MakeMKV reports as failed ("… titles saved, 1 failed") is refused even when it exits cleanly and leaves a file behind.
- On Windows, MakeMKV is also found under `C:\Program Files\MakeMKV`.
- Teletext and CEA-608 subtitle tracks, common in DVB `.ts` recordings, are left out of MKV output instead of failing the encode; the source is kept.
- TUI: saving a different encoder re-checks that FFmpeg has it, updating the Home warning and saying so when it is missing; the startup warning about a missing encoder no longer stays on screen for good.
- TUI: Ctrl+C while shutting down no longer opens a dialog that cannot be answered, and `q` on the "terminal too small" screen asks to quit even while a settings field is being edited, instead of typing into it.
- TUI: notices in Chinese no longer lose their last line, because rows for text without spaces are counted the way the terminal wraps them.
- TUI: in the disc folder picker, Space on `..` scans the open folder, as the help bar says, instead of going up a level.
- TUI: a panic in a background thread goes to the log instead of being printed over the screen.
- TUI: during an encode, the track configuration "(n/m)" counter and the switch-file hint count only the files that can still be configured.
- TUI: a long file name no longer pushes the question out of the Dolby Vision dialog.
- Web: toasts use the full screen width on phones instead of at most half of it.
- Web: queue text can be selected during an encode, and screen readers no longer re-read every row once a second.
- Web: a missing VMAF score or progress value shows "?" instead of 0.0.
- Web: track changes made while a save is still being sent are no longer dropped without asking.
- Web: going back to the drive picker in the disc dialog no longer shows the previous scan's error.
- Web: a status poll that hangs, for example after the computer sleeps, times out after 10 seconds and shows the offline banner.
- Web: dragging a text selection out of a dialog no longer closes it, or cancels a running disc scan.
- Web: Windows paths in the file browser show backslash separators, including after the drive.
- A blank `output_directory` in config.toml counts as unset instead of writing encodes to the working directory.
- `--stop`, `--status`, `--uninstall-service` and `--scan-discs` no longer create config.toml or probe the GPU when there is no configuration yet.
- When `--start-foreground` generates an access token it points to `--status` for the link, instead of to a URL it does not print.
- Settings errors from the TUI and the web UI (output folder, browse root, staging and MakeMKV paths, bind address, port, token length) and the web "no video files" error follow the interface language, and the TUI names the output-folder problem instead of showing a field label.
- Translation fixes: one term for disc ripping in Chinese, German, Italian and Spanish; "source", "encoder", "settings" and "folder" are used consistently; the Spanish autostart notice says "at login"; German daemon notices use "Sie"; the Chinese Dolby Vision profile label and track dialog are corrected.
- An unreadable `queue.json` is moved to `queue.json.unreadable-<time>` instead of being replaced by an empty queue, `queue.json.bak` is refreshed only from a queue that reads back, and both UIs say where the file was kept.
- `--install-service`, and the access token `--start` generates, refuse to run on a `config.toml` that cannot be read instead of saving defaults over it.
- TUI: finishing an analysis, encode or rip no longer switches away from Home, file selection, file review or Settings; files added on the review screen are analysed once confirmed, and a Settings field being edited stays open.
- `--stop` and `--restart` signal the daemon only while it still holds its PID file lock, and wait for that lock to be released, instead of signalling whatever process has since reused its PID.
- Web UI: closing the disc dialog, or going back in it, no longer cancels a disc scan another browser tab started.

### Known limitations

- Dolby Vision profile 7 UHD discs are extracted by MakeMKV with the enhancement layer as a separate MKV track, which FFmpeg will not recombine. Those discs encode from the HDR10 base layer and the EL/RPU track shows up as a stray stream.
- The disc test suite runs against a fake `makemkvcon`; ripping has not been checked against real hardware.

[3.0.0]: https://github.com/framicheli/av1converter/compare/v2.6.1...v3.0.0
