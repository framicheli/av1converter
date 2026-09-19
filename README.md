# AV1Converter

A tool to batch convert video files to the AV1 codec using FFmpeg. It auto-detects available hardware encoders, verifies output quality with VMAF, and manages the full encoding pipeline through a terminal UI or a local web UI.

## Features

- **Interactive TUI** — Browse files, configure tracks, and monitor encoding progress in the terminal
- **Two operating modes** — Re-encode to AV1, or demux/remux to repackage and strip tracks without recompression
- **Hardware acceleration** — Automatically detects and uses NVIDIA NVENC, Intel QSV, or AMD AMF; the choice can be overridden manually in Settings
- **Batch processing** — Convert a single file, a folder, or an entire directory tree recursively
- **Smart preset selection** — Automatically picks encoding parameters based on resolution and HDR type
- **Dolby Vision support** — Keep Dolby Vision in the AV1 output (profile 10) or convert to HDR10; profile 5 sources are tone-mapped on the GPU when converting to HDR10. True HDR10 static metadata (mastering display, MaxCLL/MaxFALL) is written for SVT-AV1 encodes; hardware encoders get PQ color tags only
- **Quality presets** — Low, Medium, or High shifts CRF/CQ values across every resolution tier at once; Custom leaves each tier's values manually editable
- **VMAF quality verification** — Scores output quality after encoding (mean and minimum sampled frame, every 10th frame); deletes the source only when both meet the threshold (never for remuxes, disabled VMAF, tone-mapped DV profile 5, or when audio was transcoded to Opus, a selected subtitle was converted or left out, cover art or attachments are not carried into the output, or the source is not 4:2:0 at 10 bits or fewer)
- **Track selection** — Auto-selects audio and subtitle tracks by preferred language (ISO 639-1/639-2 and BCP-47 tags like `en-US` are treated as aliases, e.g. `en`/`eng`); when nothing matches, audio falls back to the first track and subtitles to none unless "select all" fallback is enabled (the default)
- **Audio transcoding** — Copy audio tracks untouched (the default) or convert any of them to Opus at the source's own channel layout; per-track in both the TUI and the web UI
- **Disc ripping** — Import titles straight from a DVD or Blu-ray through MakeMKV, ripping one title while the previous one encodes (see [Disc Ripping](#disc-ripping))
- **Daemon mode with web UI** — Run headless and manage the queue from a browser (see [Daemon Mode](#daemon-mode-and-web-ui))
- **Multi-language UI** — TUI and web UI available in English (default), Italian, Spanish, French, German, and Chinese; selectable in Settings
- **Configurable** — All key settings adjustable through the built-in configuration screen, the web settings page, or `config.toml` (see [Configuration](#configuration))

## Prerequisites

`ffmpeg` and `ffprobe` must be on your `PATH`. (Ripping discs additionally needs MakeMKV — see [Disc Ripping](#disc-ripping); nothing else depends on it.) **FFmpeg 8.0 or newer is required** for Dolby Vision passthrough ("Dolby Vision profile 10 support in AV1" landed in 8.0; the tool is developed and tested against 8.1). The version is not checked at startup.

Not every FFmpeg build includes every feature this tool uses. What you need depends on which features you use:

| FFmpeg capability | Needed for | Required? |
|-------------------|-----------|-----------|
| `libsvtav1` | Software AV1 encoding; Dolby Vision passthrough (AV1 profile 10) | Yes, unless you only use hardware encoders |
| `libvmaf` | VMAF quality verification (and source auto-deletion, which depends on it) | Optional |
| `libopus` | Converting audio tracks to Opus | Optional — only if you transcode audio |
| `libplacebo` + a working Vulkan driver | Dolby Vision **profile 5** → HDR10 tone-mapping | Optional — only for DV profile 5 conversion |
| `av1_nvenc` / `av1_qsv` / `av1_amf` | Hardware encoding (needs a matching GPU driver: NVIDIA driver, Intel media driver + libvpl, or AMD AMF runtime) | Optional |

Check what your build supports:

```bash
ffmpeg -version                                  # 8.0+ required for DV passthrough
ffmpeg -h encoder=libsvtav1 | grep dolbyvision   # DV passthrough
ffmpeg -filters | grep libvmaf                   # VMAF verification
ffmpeg -encoders | grep libopus                  # Opus audio transcoding
ffmpeg -filters | grep libplacebo                # DV profile 5 tone-mapping
vulkaninfo --summary                             # Vulkan driver (DV profile 5 only)
```

Platform notes:

- **Arch Linux** — `pacman -S ffmpeg` includes all of the above. For DV profile 5 tone-mapping also install your GPU's Vulkan driver (`vulkan-radeon`, `vulkan-intel`, or `nvidia-utils`).
- **Debian/Ubuntu** — the distro `ffmpeg` includes `libsvtav1`, but is often older than 8.0 (no DV passthrough — selecting "keep Dolby Vision" will fail); `libvmaf` and `libplacebo` also vary by release. Verify with the commands above.
- **Fedora** — use the FFmpeg from RPM Fusion; the freeworld build includes `libsvtav1` and `libvmaf`.
- **macOS** — `brew install ffmpeg` includes `libsvtav1` and `libvmaf` but **not** `libplacebo`/Vulkan: everything works except DV profile 5 → HDR10 tone-mapping (keeping DV still works for profile 5).
- **Windows** — the full builds from [gyan.dev](https://www.gyan.dev/ffmpeg/builds/) or [BtbN](https://github.com/BtbN/FFmpeg-Builds/releases) (GPL variant) include everything.

## Installation

### Cargo

```bash
cargo install av1converter
```

### Nix

The flake builds the binary only; `ffmpeg` and `ffprobe` still have to be on your `PATH`.

Run without installing:

```bash
nix run github:framicheli/av1converter
```

Or install into your Nix profile:

```bash
nix profile install github:framicheli/av1converter
```

### Homebrew

macOS (Apple Silicon and Intel) and x86_64 Linux; installs the prebuilt release binary and pulls in `ffmpeg`.

```bash
brew tap framicheli/tap
brew install av1converter
```

The formula lives in [framicheli/homebrew-tap](https://github.com/framicheli/homebrew-tap) and is updated automatically by the release workflow.

### Arch Linux (AUR)

After the package is published to the AUR:

```bash
git clone https://aur.archlinux.org/av1converter.git
cd av1converter
makepkg -si
```

### Debian, Ubuntu, and Fedora

Download the package for your release from the [release page](https://github.com/framicheli/av1converter/releases), then install it:

```bash
sudo apt install ./av1converter_VERSION_amd64.deb
sudo dnf install ./av1converter-VERSION-1.x86_64.rpm
```

On Fedora the RPM depends on the `ffmpeg` package from RPM Fusion; enable RPM Fusion first (see Platform notes above).

### Release binary or source

Release archives are checksummed in `SHA256SUMS`. Extract the target-suffixed executable, rename it to `av1converter` (or `av1converter.exe` on Windows), and put it somewhere on your `PATH`.

To build from source:

```bash
git clone https://github.com/framicheli/av1converter.git
cd av1converter
cargo build --release
```

The compiled binary will be at `target/release/av1converter`.

### Uninstallation

Stop a background daemon before removing the binary, and drop login autostart if you enabled it:

```bash
av1converter --stop
av1converter --uninstall-service
```

To also delete configuration and daemon state (queue, logs, access token), run:

```bash
av1converter --purge
```

It refuses while the daemon is running, lists the directories (and the login service, if installed) it will remove, and asks `Are you sure? [y/N]`; answering anything but yes changes nothing. The binary, encoded files, and a custom disc staging directory are left alone. Package uninstall does not delete user data, so `--purge` has to run while the binary is still installed.

Then use the same tool that installed it:

```bash
cargo uninstall av1converter
nix profile remove av1converter
brew uninstall av1converter
sudo pacman -R av1converter
sudo apt remove av1converter
sudo dnf remove av1converter
```

For a manual installation, remove the binary you placed on `PATH`.

## Usage

```bash
./av1converter
```

With no arguments it starts the TUI. The options below control the web-UI daemon and maintenance tasks.

```
Usage: av1converter [OPTION]

  (no option)          start the interactive TUI
  --start              start the web-UI daemon in the background (Unix; must be enabled in Settings)
  --start-foreground   start the daemon in the foreground, logging to stdout
  --restart            stop the daemon gracefully, then start it again (Unix)
  --stop               stop the background daemon (Unix)
  --status             show whether the daemon is running
  --install-service    start the daemon at login (Linux/macOS)
  --uninstall-service  stop starting the daemon at login
  --scan-discs         list optical drives and the titles on the loaded disc
  --purge              delete configuration and daemon state after confirmation
  --help               show this help
  --version            show the version

Legacy aliases: --daemon is --start; --daemon-foreground is --start-foreground
```

`-h` and `-V` are short for `--help` and `--version`. An unknown option, or any extra argument, prints the usage and exits with status 2; a near-miss such as `--stpo` also gets a suggestion.

`--scan-discs` is a diagnostic: it prints what MakeMKV reported, so a disc that lists oddly can be seen rather than guessed at.

### Workflow

1. **Home menu** — Open a single file, a folder, or a folder recursively; rip a DVD or Blu-ray; or go to Configuration. Encodes and analysis use the saved configuration; edits on the Configuration screen take effect once saved
2. **Selection** — Navigate with arrow keys. For a file, `Enter` picks it; for a folder, `Enter` opens it and `Space` selects the highlighted subfolder, or the open folder when the cursor is on `..` or a file. For a disc, pick the drive (skipped when there is only one), or open a disc folder or `.iso` image, then toggle titles with `Space`; extraction feeds the same steps below
3. **File review** — Confirm the list of files found (multiple files or a folder only; a single file skips this step)
4. **Analysis** — Each file is probed for its streams, resolution and HDR format
5. **Track configuration** — Select audio and subtitle tracks to include, and switch the per-file mode (encode or demux/remux) with `r`. Encoding starts as soon as the first file is confirmed; the others can be configured while it runs, or all at once with `A`
6. **Encoding and VMAF verification** — Monitor per-file and overall progress; `a` adds more files, folders or a disc while work runs, and `Esc` asks before cancelling; when a rip, analysis and encode run together, `Esc` cancels the rip, `A` the analysis and `E` the encode. When analysis or encoding finishes, the TUI moves on to Track configuration or Finish only from the Queue, Track configuration or Finish screens; any other screen stays open. Cancelling during analysis stops only the analysis; files already analysed stay queued. Mean and minimum sampled-frame VMAF scores are computed after each file; the source is deleted only if `delete_source_on_success` is on and both meet the threshold (encode mode only, never when audio was transcoded, Dolby Vision set to be kept was converted to HDR10 by a hardware encoder, a selected subtitle was converted or left out, cover art or attachments are not carried over, the source has extra video or data streams, is Dolby Vision profile 7 or is not 4:2:0 at 10 bits or fewer, or the job was cancelled)
7. **Finish** — View a summary of conversions, skipped files, and space saved; `Enter` starts a new conversion after a confirmation, `Esc` returns to the queue

## Modes

Each file in the queue is processed in one of two modes. The mode is chosen per file on the track configuration screen and can be toggled with `r`.

### Encode Mode (Encode Video → AV1)

The default mode for non-AV1 sources. The video stream is re-encoded to AV1 using the detected hardware or software encoder, with parameters chosen automatically from the resolution and HDR format (see [Encoding Presets](#encoding-presets)). Selected subtitle tracks are copied when the output container supports their format, otherwise compatible text subtitles are converted; audio tracks are copied unless you convert them to Opus (see [Audio Transcoding](#audio-transcoding)). Output is written to the configured container (`mkv`, `mp4` or `webm`) with the configured suffix (default `_av1`), and the result is verified with VMAF before the source can be deleted. MKV output keeps attachments such as embedded fonts and cover art. Subtitles the container cannot hold (bitmap subtitles in WebM, bitmap subtitles other than DVD in MP4) are left out, and WebM output converts any audio that is not already Opus or Vorbis to Opus, which also keeps the source from being auto-deleted.

### Demux/Remux Mode (Remux Only → Copy Video)

A fast, lossless repackaging mode that copies the video, audio, and subtitle streams without recompression. Use it to drop unwanted audio/subtitle tracks or clean up files that are already AV1 — no quality is lost and the operation is near-instant since nothing is re-encoded. The output keeps the source file's container extension and uses the `_remux` suffix. Because no video encoding happens, VMAF verification is skipped.

Audio is the one exception: tracks you convert to Opus are re-encoded even here, so an already-AV1 file with an oversized lossless track can be shrunk without touching the video.

Files that are already encoded in AV1 default to demux/remux mode automatically; everything else defaults to encode mode.

### Dolby Vision Handling

When a Dolby Vision source is queued for encoding with SVT-AV1, a dialog asks how to convert it (reopen it anytime with `d`). Hardware encoders cannot write the RPU, so with NVENC, QSV or AMF the output is HDR10 without asking:

1. **AV1 with Dolby Vision (profile 10)** — the DV dynamic metadata (RPU) is carried into the AV1 stream. Requires the SVT-AV1 encoder; hardware encoders (NVENC/QSV/AMF) cannot write the RPU and always produce HDR10. For cross-compatible profiles (7/8) the HDR10 base layer is preserved, so players without DV support still get correct HDR10 playback.
2. **AV1 with true HDR10** — the DV layer is dropped and the HDR10 static metadata (mastering display, MaxCLL/MaxFALL) from the source is written into the AV1 stream when encoding with SVT-AV1. Hardware encoders still convert to PQ HDR10 color tags, but do not currently re-attach mastering/CLL SEI.

**Profile 5** sources (IPT-PQ-c2, no HDR10-compatible base layer) are special: converting to HDR10 tone-maps the video on the GPU via `libplacebo` (requires Vulkan), while keeping DV produces output that only plays correctly on DV-capable players. HDR10 conversion is the recommended default for profile 5; keeping DV is recommended for profiles 7/8. VMAF verification is skipped for tone-mapped profile 5 output, since the pixels are intentionally changed. Sources that carry Dolby Vision side data without a readable `dv_profile` are rejected at analysis so a missing profile cannot skip tonemap and produce wrong colors.

HDR10+ dynamic metadata is not preserved; only static HDR10 mastering/CLL (on SVT-AV1) and PQ/HLG color tags are carried forward.

### Audio Transcoding

Audio tracks are copied untouched by default. Any track can instead be converted to Opus, and the choice is per track: a file can keep its lossless track and shrink the commentary, or the other way round.

The channel count and order are preserved — uncommon layouts use independent Opus mapping instead of being silently downmixed — and the bitrate follows the channel count, at 64 kbps per channel by default:

| Source track | Opus output |
|--------------|-------------|
| Mono | 64 kbps |
| Stereo | 128 kbps |
| 5.1 | 384 kbps |
| 7.1 | 512 kbps |

Set the allowance per channel with `opus_bitrate_per_channel` (16–256 kbps) to shift the whole table at once. Tracks that are already Opus are left alone rather than re-encoded, unless you turn `skip_already_opus` off.

In the TUI, press `o` on a track in the track configuration screen (`O` applies it to every selected track); the row shows the resulting bitrate, e.g. `[~] 0: eng (DTS 5.1) → OPUS 384k`. In the web UI, a centered track configuration dialog opens automatically after analysis; the **Tracks** button reopens it for any queued job that has not started encoding yet. `audio.default_mode` sets what newly queued files start out as, which is what the daemon uses when nobody configures a job by hand.

Two things worth knowing:

- **Sources are never auto-deleted when something besides the video changes.** `delete_source_on_success` relies on a VMAF score, and VMAF compares video only. Jobs that transcode audio, lose Dolby Vision that was set to be kept (hardware encoders cannot write it and convert to HDR10), convert or leave out a selected subtitle (bitmap subtitles in MP4/WebM, ASS styling turned into `mov_text` or WebVTT), drop cover art or attachments (every container except MKV drops attachments; cover art is dropped everywhere), or drop streams that cannot be selected (a second video stream, data streams such as timecode or GPS tracks, a Dolby Vision profile 7 enhancement layer), or reduce the source's chroma or bit depth (everything is encoded as 4:2:0 10-bit, so 4:2:2, 4:4:4, RGB, alpha and 12-bit sources lose detail VMAF does not measure) keep the source and log why.
- **Object-based audio does not survive.** Converting a TrueHD Atmos or DTS:X track to Opus keeps the channel bed and discards the object metadata. Copy those tracks if you want them intact.

### Keyboard Controls

| Key | Where | Action |
|-----|-------|--------|
| `↑` / `k`, `↓` / `j` | Everywhere | Navigate |
| `Enter` | Everywhere | Select / Confirm; on the queue, configure the next job waiting for tracks, or open the summary once nothing is running |
| `a` | Disc titles | Select every title, or clear them |
| `Space` | Selection, disc titles, track config | Select the highlighted or open folder, toggle a disc title, toggle the highlighted track |
| `Esc` | Everywhere | Go back; on the queue, cancel the disc rip, or else the analysis, or else the encode (asks first) |
| `PgUp` / `PgDn` | Queue, finish, disc titles | Scroll the detail pane |
| `K` | Queue | Move the highlighted waiting job up |
| `a` | Queue | Add files, folders or a disc (opens Home; `Esc` on Home returns to the queue) |
| `t` | Queue | Open the tracks of the highlighted job, if it is not being encoded |
| `x` / `Delete` | Queue | Remove the highlighted waiting or finished job (a ripped title asks first) |
| `C` | Queue | Clear finished jobs while nothing runs |
| `A` / `E` | Queue | Cancel the analysis / the encode while other work also runs (asks first) |
| `Tab` / `Shift+Tab` | Track config | Cycle focus forward / backward: audio → subtitles → Continue |
| `←` / `h`, `→` / `l` | Track config | Previous / next file |
| `r` | Track config | Switch mode: encode ↔ demux/remux |
| `d` | Track config | Change Dolby Vision handling (DV sources) |
| `a` / `s` | Track config | Toggle all audio / subtitle tracks |
| `o` | Track config, audio focused | Toggle Opus for the highlighted audio track |
| `O` | Track config | Convert all selected audio tracks to Opus; press again to undo |
| `A` | Track config | Apply these choices to every file still waiting, matching tracks by order |
| `←` / `h`, `→` / `l` | Configuration | Decrease / increase the value |
| `Enter` | Configuration | Edit a text field (`Enter` commits, `Esc` aborts) |
| `s` | Configuration | Save configuration |
| `1` / `2` | Dolby Vision dialog | Pick an option; `↑`/`↓` or `Tab` then `Enter` also work. `Esc` keeps the current choice, or applies the recommended one when there is none yet |
| `y` / `n` | Confirmation dialogs | Answer yes / no |
| `←` / `h`, `→` / `l`, `Enter` | Confirmation dialogs | Highlight Yes or No, then answer with the highlighted one (No unless changed) |
| `q`, `Ctrl+C` | Everywhere | Quit (with confirmation, which mentions unsaved configuration changes; `q` is ignored while editing a text field) |

## Disc Ripping

Titles can be imported straight from a DVD or Blu-ray: pick **Rip DVD / Blu-ray** on the home menu, or **+ Disc** in the web UI. Selected titles are extracted one at a time to a staging directory, then analyzed, track-configured and encoded exactly like a file you opened yourself. Ripping and encoding overlap — the next title reads from the disc while the previous one encodes — so peak disk use stays at one rip plus one encode.

Ripping needs **MakeMKV**, which is not bundled:

| Platform | What to install | Where `makemkvcon` ends up |
|---|---|---|
| macOS | The `.dmg` from [makemkv.com](https://www.makemkv.com/) | Inside the app bundle, at `/Applications/MakeMKV.app/Contents/MacOS/makemkvcon`. The GUI never has to be opened. |
| Linux | `makemkv-oss` + `makemkv-bin` from source, or a distro package (AUR `makemkv`, the Ubuntu PPA) | On `PATH`. The GUI is optional: `./configure --disable-gui` skips Qt entirely. Your user must be in the `cdrom` group. |
| Windows | The installer from makemkv.com | `C:\Program Files (x86)\MakeMKV\` |

Set `makemkvcon_path` under `[disc]` if it lives somewhere else — a Flatpak install, for instance, needs a small wrapper script since it is run through `flatpak run`. The path must name a file called `makemkvcon` or `makemkvcon64` (with or without `.exe`), so name the wrapper `makemkvcon`. Set `staging_directory` to a scratch drive: a Blu-ray title needs 100 GB or more, and the staging file is deleted once its encode succeeds (it is kept when the encode fails, scores under the VMAF threshold, or its VMAF check cannot run). Staging directories left behind by an interrupted rip are removed when the TUI or the daemon starts and whenever a disc run ends. Only the folders AV1Converter created for a rip are removed; anything else in the staging directory is left alone. An existing output directory must be configured before a rip can start (a disc job cannot write next to its source, which is the staging directory); disc jobs write there even when `same_directory` is on.

Besides a physical drive, a disc folder (`VIDEO_TS` / `BDMV`) or an `.iso` image can be opened. Titles shorter than 60 seconds are hidden, and output is named `<disc label>_t<NN>` plus the configured suffix.

DVD decryption is free permanently. Blu-ray needs a purchased MakeMKV licence or the free beta key, and **that key expires every couple of months** — refresh it in MakeMKV when the app reports it as expired. That expiry is MakeMKV's, not this tool's.

**Known limitation — Dolby Vision profile 7 (UHD Blu-ray).** MakeMKV extracts profile 7 discs with the Dolby Vision enhancement layer as a *separate* MKV track, and FFmpeg will not recombine it. Those discs encode from the HDR10 base layer, and the EL/RPU track shows up as a stray stream on the track screen. This is a MakeMKV/FFmpeg limitation, not a bug in the conversion.

## Daemon Mode and Web UI

The daemon runs headless with an embedded web UI for managing conversions from a browser: a dashboard with live progress, the queue (add files or whole folders through a server-side file browser, import titles from a disc in the machine's own drive, cancel the encode, the analysis or a rip separately, remove, clear finished jobs), and a settings page. The queue is persisted, so jobs still waiting when the daemon stops are restored on the next start. Track selection and Dolby Vision handling are resolved automatically, using your configured language preferences and encoder. After analysis finishes, a centered dialog opens for the per-file choices; **Apply to remaining files** copies them to the other waiting jobs by track order, while extra tracks keep their automatic defaults and the Dolby Vision choice goes only to files of the same Dolby Vision profile. Closing the dialog without saving stops the automatic prompts until new files are added. Removing a ripped job, or clearing finished jobs that include one, asks first, because its rip file is deleted.

Enable it in Settings (or set `enabled = true` under `[daemon]`), then:

```bash
av1converter --start       # start in the background
av1converter --restart     # stop cleanly, then start again
av1converter --status      # is it running, and where
av1converter --stop        # stop it; an active encode starts over on the next start
```

To start it at login, turn on **Run at Startup** in Settings, or:

```bash
av1converter --install-service    # systemd user unit (Linux) or launchd agent (macOS)
av1converter --uninstall-service  # remove it
```

`--install-service` also sets `enabled = true`, generates a token if needed, and starts the daemon right away when it is not already running. The systemd unit and the launchd agent record your `PATH` at install time, so an FFmpeg installed through Homebrew or another package manager is found; re-run `--install-service` (or turn Run at Startup off and on) after upgrading from an older version or moving FFmpeg.

`--stop` lasts until the next login; uninstall is what prevents it coming back. On a headless Linux machine the user unit dies at logout unless lingering is enabled (`loginctl enable-linger $USER`). Re-run `--install-service` after moving the binary so `ExecStart` stays correct.

Turning **Run at Startup** off from the web settings page leaves the current daemon session running and prevents it from starting at the next login. On macOS the launchd job is also disabled, so a crash does not relaunch it.

Both `--stop` and `--restart` stop an active encode cleanly and keep the queue: the file that was encoding starts over, and the jobs behind it stay queued for the next start. `--stop` waits up to 45 seconds. `--start` returns once the daemon is listening, and reports a failure (a port already in use, for example) instead of claiming success. `--start`, `--stop` and `--restart` are Unix-only; on Windows run `--start-foreground` and stop it with `Ctrl+C`. The old `--daemon` and `--daemon-foreground` spellings remain available as compatibility aliases. `--start` logs to `$XDG_DATA_HOME/av1converter/daemon.log` (default `~/.local/share/av1converter/daemon.log`); `--start-foreground` and the systemd unit log to stdout (the journal).

The queue is saved to `queue.json` in the same directory, with the previous readable save kept as `queue.json.bak`. A `queue.json` that cannot be read is moved to `queue.json.unreadable-<time>` and the queue reloads from `queue.json.bak`; the TUI and the web UI show where the file was kept. The TUI does not persist its queue.

### Security

The web UI can browse the filesystem, start encodes and rewrite the configuration, so it binds to `127.0.0.1` and is guarded by an access token.

**The token is generated for you.** If `auth_token` is empty or shorter than 32 bytes when the daemon starts, a random 128-bit token is minted and saved to `config.toml`, and `--start` prints the URL that carries it:

```
Web UI listening on http://127.0.0.1:8399/#token=aa2006351b5214a820e3fdc64da870af
```

Open that link once and the browser keeps the token for the tab's session; `av1converter --status` prints it again whenever you need it (use it after `--start-foreground`, which prints the URL without the token). Adding `#token=…` to the address of a page that is already open works too. The fragment after `#` is not sent to the HTTP server or included in HTTP logs. Clearing or weakening `auth_token` does not disable authentication — a strong token is generated on the next start.

Two things are worth knowing before exposing the daemon to a network:

- `browse_root` — set it. It is the only directory the file browser and the queue will accept paths under, and it is the difference between "manage my media library" and "read every file this user can read". An empty `browse_root` is refused when the daemon binds outside loopback.
- `bind_address` — leave it on loopback unless you mean it. A non-loopback bind is refused unless `allow_insecure_lan = true` (plain HTTP would otherwise expose the Bearer token on the LAN); even with that opt-in, prefer an HTTPS reverse proxy on a trusted network.

The daemon serves plain HTTP. If it must be reachable beyond the local machine, put it behind an HTTPS reverse proxy with connection/request timeouts and rate limiting, and keep the direct daemon port firewalled from untrusted networks. The embedded server is intended for trusted local or LAN use, not direct internet exposure.

Every configuration field exists in both the TUI and web settings pages; the TUI hides rows that do not apply (per-tier values outside `custom`, VMAF options while VMAF is off, `film_grain` unless the encoder is SVT-AV1) and the web page greys them out or omits them. Settings that can widen host access or select an executable — the `[daemon]` and `[disc]` blocks — are writable in the web UI only when it is opened through a loopback origin such as `http://127.0.0.1:8399/` or `http://localhost:8399/`. They remain visible but read-only to LAN and reverse-proxy clients; a request carrying `X-Forwarded-For`, `X-Forwarded-Host`, `X-Real-IP` or `Forwarded` counts as remote. A reverse proxy on the same host that adds none of those headers (nginx's default `proxy_pass`, for example) looks like a loopback browser: set `behind_proxy = true` when one forwards to the web UI, and every web request then counts as remote. `makemkvcon_path` only accepts a file named `makemkvcon` or `makemkvcon64` (with or without `.exe`). The web Settings tab re-reads the configuration each time it opens, unless it has unsaved edits. Bind-address and port changes take effect after a daemon restart. Encoder, quality and output changes update waiting jobs; track defaults apply only to files added after the change.

The current access token is never returned to a browser. A local web session can leave the token field blank to keep it or enter a replacement containing at least 32 characters. The accepted replacement becomes the browser session token immediately. The TUI's Settings screen takes the same rule for a replacement; a blank token there is regenerated on the next start.

The token is also what stops a website you visit from reaching the daemon. A page that re-points its own hostname at `127.0.0.1` still cannot produce the bearer token. The API also requires JSON for mutations, rejects unsafe unauthenticated hostnames, caps request bodies, and confines restored as well as newly added jobs to `browse_root`.

## Encoding Presets

Presets are selected automatically based on resolution and HDR format. Classification uses the short and long sides (so portrait 1080×1920 matches landscape 1920×1080):

- **SD** — below HD (e.g. 854×480)
- **HD** — long ≥ 1280 or short ≥ 720, and not yet Full HD (e.g. 1280×720, 1280×800, 1366×768)
- **Full HD** — long ≥ 1920 or short ≥ 1080, up to (but not including) UHD; **1440p / QHD uses the 1080p preset matrix**
- **UHD** — long ≥ 3000 or short ≥ 1800
- **Above 4K** — long ≥ 4097 or short ≥ 2161 (uses the 4K presets)

| Resolution | HDR          | Preset         | VMAF Model         |
|------------|--------------|----------------|--------------------|
| SD (e.g. 480p/576p) | Any | **SD** | vmaf_v0.6.1 (HDR: vmaf_v0.6.1neg) |
| HD (720p, laptop 16:10, …)  | Any          | **HD**         | vmaf_v0.6.1 (HDR: vmaf_v0.6.1neg) |
| 1080p / 1440p      | No           | **1080p SDR**  | vmaf_v0.6.1        |
| 1080p / 1440p      | Yes (HDR10/HLG) | **1080p HDR**  | vmaf_v0.6.1neg     |
| 1080p / 1440p      | Dolby Vision    | **1080p DV**   | vmaf_v0.6.1neg     |
| 4K         | No              | **4K SDR**     | vmaf_4k_v0.6.1     |
| 4K         | Yes (HDR10/HLG) | **4K HDR**     | vmaf_4k_v0.6.1neg  |
| 4K         | Dolby Vision    | **4K DV**      | vmaf_4k_v0.6.1neg  |

Sources above 4K use the 4K presets. The 4K VMAF models are used when the longer side is at least 3840 pixels (portrait 2160×3840 included); narrower 4K-tier sources use the 1080p models. Dolby Vision counts as HDR for model selection. VMAF scores every 10th frame (`n_subsample=10`); source auto-deletion requires both the mean and the minimum sampled frame score to meet `vmaf_threshold`.

Files already encoded in AV1 default to demux/remux mode instead of being re-encoded.

## Encoder Detection

On first run (when no `config.toml` exists yet) the tool encodes one test frame with each hardware AV1 encoder, in this order, and picks the first that succeeds. A test encode that takes longer than 10 seconds counts as a failure. Change the encoder in Settings afterwards:

1. **NVIDIA NVENC** (`av1_nvenc`), e.g. RTX 40/50 series
2. **Intel Quick Sync** (`av1_qsv`), e.g. Arc GPUs
3. **AMD AMF** (`av1_amf`), e.g. RX 7000 series
4. **SVT-AV1** (`libsvtav1`), the software fallback when no hardware encoder works (always on macOS)

## Configuration

Configuration is stored at `$XDG_CONFIG_HOME/av1converter/config.toml` (default `~/.config/av1converter/config.toml`). On Windows it is `%APPDATA%\av1converter\config.toml` unless `XDG_CONFIG_HOME` or `HOME` is set (Git Bash and MSYS2 set `HOME`), in which case the paths above apply. It can be edited directly or through either settings interface. The first run writes a complete file. The excerpt below leaves out the `[presets.*]` tables, so edit the generated file rather than replacing it with this block.

```toml
language = "en"                # UI language: en, it, es, fr, de, zh (English if omitted)
encoder = "SvtAv1"             # Selected encoder: Nvenc, Qsv, Amf, SvtAv1 (auto-detected on first run, then overridable)
quality_preset = "medium"      # Quality preset: low, medium, high, custom (new files get medium; a file without this key loads as custom)

[quality]
vmaf_threshold = 90.0          # Mean and min sampled-frame VMAF must both meet this (0–100)
vmaf_enabled = true            # Enable/disable VMAF verification after encoding
delete_source_on_success = false  # Delete source when mean and min VMAF both meet the threshold

[performance]
svt_preset = 4             # SVT-AV1 preset: 0 (slowest) – 13 (fastest)
nvenc_preset = "p4"        # NVENC preset: p1 (best quality) – p7 (fastest)

[output]
suffix = "_av1"            # Appended to output filenames
container = "mkv"          # Output container: mkv, mp4 or webm (anything else falls back to mkv)
same_directory = true      # Write output next to source file
# output_directory = "/path/to/output"  # Existing directory; required when same_directory = false and for disc rips

[tracks]
preferred_audio_languages = ["eng", "ita"]
preferred_subtitle_languages = ["eng"]
select_all_fallback = true # Default: select all tracks if no preferred language is found (set false for first-audio / no-subs fallback)

[audio]
default_mode = "copy"          # What newly queued files start as: "copy" or "opus"
opus_bitrate_per_channel = 64  # kbps per channel (16–256); stereo → 128k, 5.1 → 384k
skip_already_opus = true       # Leave tracks that are already Opus alone

[daemon]
enabled = false            # Required before `--start` will start the daemon
bind_address = "127.0.0.1" # Loopback by default; see the security note below
port = 8399
browse_root = ""           # Confine the web file browser to this directory ("" = whole filesystem; required when binding off-loopback)
auth_token = ""            # API secret (empty or under 32 bytes = regenerate on next start)
allow_insecure_lan = false # Required to bind off-loopback over plain HTTP
behind_proxy = false       # Treat every web request as remote; set when a reverse proxy on this host forwards to the web UI

[disc]
# makemkvcon_path = ""      # Unset: PATH, then the platform's MakeMKV install location
# staging_directory = ""    # Where ripped titles wait to be encoded (unset = a private per-user folder under the system temp directory)
```

If `config.toml` cannot be parsed it is left untouched and defaults are used for that run. Saving settings afterwards first copies the broken file to `config.toml.bak`. `--install-service`, and `--start` when it has to generate an access token, refuse to run until the file is fixed or removed.

Each resolution preset exposes per-encoder quality values (`crf`, `nvenc_cq`, `qsv_quality`, `amf_quality`). `film_grain` synthesis strength is SVT-AV1 only and is hidden in the settings UI when a hardware encoder is selected.

`quality_preset` controls how those per-resolution values are managed: `low`, `medium`, and `high` apply built-in values across every tier at once (the `[presets.*]` tables are rewritten whenever the configuration is loaded or saved), while `custom` leaves the complete preset matrix editable in either settings interface or the configuration file.

When `same_directory` is disabled, `output_directory` is required and must be an existing directory; a leading `~` is expanded. A saved directory that has since disappeared does not block saving other settings: the save goes through with a warning. It can be entered in the TUI configuration screen or edited directly in the file; the web settings page also exposes it within `browse_root`. In the web UI, a new or changed `output_directory` must exist and lie inside `browse_root` even while `same_directory` is on, since disc rips always write there; it cannot be cleared while ripped titles are waiting to encode.

## Debugging

Set the `AV1_DEBUG` environment variable to enable log output:

```bash
AV1_DEBUG=1 ./av1converter
```

Logs roll daily and are written to:
- **macOS/Linux:** `$XDG_DATA_HOME/av1converter/av1converter.log.<date>` (default `~/.local/share/av1converter/`)
- **Windows:** `%LOCALAPPDATA%\av1converter\av1converter.log.<date>`, unless `XDG_DATA_HOME` or `HOME` is set, in which case the macOS/Linux path applies

For the daemon, `AV1_DEBUG` raises the log level of stdout or `daemon.log` to debug.
