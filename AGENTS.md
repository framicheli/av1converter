# AGENTS.md

This file provides guidance to coding agents when working with code in this repository.

## Project

**AV1Converter** batch converts video to AV1 with FFmpeg. One Rust binary has two front ends over the same pipeline: a ratatui TUI and a headless daemon with an embedded web UI. Its core promise is that **a source file is deleted only when the output is proven good**, meaning VMAF mean and minimum both meet the threshold and every selected stream made it into the output unchanged. Any change that touches deletion must keep that guarantee; when in doubt, keep the source.

Runtime dependencies: `ffmpeg`/`ffprobe` 8.0+ on `PATH` (features vary by build: `libsvtav1`, `libvmaf`, `libopus`, `libplacebo`, hardware AV1 encoders). Disc ripping also needs MakeMKV's `makemkvcon`. The README's Prerequisites section has the full matrix.

## Commands

CI (`.github/workflows/rust.yml`) runs these on Linux, macOS and Windows; run them before committing:

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -W clippy::pedantic -D warnings
cargo test --all-targets
cargo check --locked --all-targets   # MSRV job, toolchain 1.91
```

- Single test: `cargo test <substring>`, e.g. `cargo test web_keys_are_unique`.
- Run: `cargo run` (TUI), `cargo run -- --start-foreground` (daemon + web UI, logs to stdout). `AV1_DEBUG=1` enables debug logging.
- Tests are inline `#[cfg(test)]` modules; there is no `tests/` directory.
- `src/encoder/end_to_end.rs` encodes real files with the `ffmpeg` on `PATH` and skips (not fails) when the build lacks a feature.
- Disc tests run against a fake `makemkvcon` shell script (`src/disc/testing.rs`), so they are `#[cfg(unix)]`.

## Architecture

`src/main.rs` parses the single CLI option (see `USAGE`) and either runs the TUI or the daemon.

**Pipeline** (shared): `analyzer` (ffprobe → `VideoMetadata`, resolution tier, HDR/Dolby Vision) → `queue::job` (`EncodingJob`, track auto-selection, DV mode, output paths) → `queue::worker::run_worker` on a thread → `encoder::command_builder` builds the ffmpeg args, `encoder::ffmpeg` runs them and parses progress → `verifier::vmaf` scores the output and decides whether the source may be deleted. The worker reports back through `WorkerMessage` over an mpsc channel.

**TUI**: `src/app.rs` holds `App` (screen state machine, `Screen` enum) and spawns the worker; `src/main.rs` owns the event loop and key handlers; `src/ui/*` are the per-screen renderers.

**Daemon**: `daemon::run_daemon` (`src/daemon/mod.rs`) is the orchestrator loop: one prober thread, the encode worker, and disc runs all report on channels into state behind `SharedState` (`Arc<Mutex<DaemonState>>`). `daemon::server` is a `tiny_http` server with the routes; `daemon::api` holds the JSON handlers. The web UI (`src/daemon/web/`: plain `index.html`, `app.js`, `style.css` and `favicon.png`, no build step) is embedded with `include_str!`/`include_bytes!`. `DaemonQueue` gives jobs stable ids; the queue persists to `queue.json`. `lifecycle` handles pid/log files and background start/stop; `service` installs the systemd/launchd login unit.

**Disc ripping** (`src/disc/`): wraps `makemkvcon` robot mode (`robot.rs` parses its output), rips titles into a staging dir (`staging.rs`), then feeds them into the normal queue as jobs.

**Config** (`src/config/`): `AppConfig` in `config.toml`. `settings::SERIALIZED_SETTING_PATHS` lists every serialized leaf. A test checks the TUI's `CONFIG_ITEMS` (`ui/config_screen.rs`) covers it; the web gets it via `setting_paths` in the settings API, and its fields live in `settingsFields` in `app.js`. A new setting needs all three.

**i18n** (`src/i18n/mod.rs`): every user-facing string is a `Msg` variant resolved by `t(lang, msg)` with an exhaustive match over six languages. Strings the web UI uses must also be listed in `WEB_KEYS`; tests check keys are unique, translated, and keep their placeholders.

**Daemon security**: the web UI can browse the filesystem, start encodes and rewrite the config, so it binds to `127.0.0.1` behind a ≥32-char access token. A non-loopback bind requires `allow_insecure_lan` and a `browse_root`. Every filesystem path from a request must stay inside `browse_root` (`api::within_root`). Don't loosen any of these.

## Operation rules

- **Commit locally, never push.** Work on the current branch.
- **Run `cargo fmt` on every commit.** If the tree has already drifted from `cargo fmt`, land a separate `style:` commit first.
- **Commit messages**: `feat|fix|docs|style: <plain description> (<files touched>)`, one logical change per commit.
- **CHANGELOG**: every user-visible change adds a line to `CHANGELOG.md` under the current version heading (`### Added` / `### Changed` / `### Fixed`, plus `### Breaking changes` when config or CLI behaviour changes incompatibly) in the same commit. Write it for users, not contributors.
- **README**: when behaviour, keys, CLI options or config keys change, update `README.md` in the same change.
- **TUI/web feature parity**: any feature added to one UI goes into the other in the same batch. The only accepted difference is that a remote browser sees host-level settings (`daemon.*`, `disc.*`, autostart) read-only; a browser on the daemon host gets everything the TUI has.
- **Strings**: no hard-coded user-facing text. Add a `Msg` variant with all six translations, and a `WEB_KEYS` entry if the web UI shows it.
- **Comments are always descriptions, never reasoning.** Say what the code does, not why it was written that way: no "because…", "so that…", "to avoid…" clauses, no design rationale, no history, and no `ponytail:` or similar markers. Reasoning goes in the commit message.
  - Good: `/// Number of HTTP worker threads.`
  - Bad: `/// Number of HTTP worker threads, so a slow scan does not stall the dashboard poll.`
  - Use doc comments on items and short `//` lines, and write test names as sentences.
- **Plans**: `.claude/plans/` (local, untracked) holds pending work plans. Check them before starting related work, and don't duplicate an item another plan owns.

## Implementation discipline

### Audit before closing a step

Before calling a change done, list what it touched and check each of these:
- Is the change in both UIs (key handler + renderer in the TUI; `api.rs` route + `app.js` in the web)?
- Is every new string translated, and in `WEB_KEYS` if the web uses it?
- Does a new setting appear in `SERIALIZED_SETTING_PATHS`, `CONFIG_ITEMS` and `settingsFields`?
- Can the change reach source deletion, `browse_root` checks, token handling, or queue persistence? If so, test the failure path too.
- Are the CHANGELOG and README updated?

Fix anything you find, then audit again. A step is closed only when an audit finds nothing real.

### Tests must be able to fail

A test that stays green when its feature is deleted or inverted is worse than none. Ask: *if I deleted or inverted the code this test names, would it fail?*
- Assert the exact value or variant, not a loose threshold; use `assert!(matches!(..))`, never a bare `matches!`.
- Test the error path and the boundary, not just the happy path. For persistence (`queue.json`, `config.toml`), write, reload and assert; then corrupt the file and assert it fails cleanly instead of silently defaulting.
- Skip only when a dependency is really missing (as `end_to_end.rs` does for FFmpeg features). When the dependency is present, a failure must fail the test.
- Test the shipped function, not a copy of it inside the test.
- No wall-clock assertions and no fixed sleeps to wait for startup; poll for the condition instead.
- When you change a behaviour on purpose, update the test to check the new behaviour. Don't weaken it just so it passes.
