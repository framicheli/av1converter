# AV1Converter

A terminal-based interactive tool to batch convert video files to the AV1 codec using FFmpeg. It auto-detects available hardware encoders, verifies output quality with VMAF, and manages the full encoding pipeline through a TUI.

## Features

- **Interactive TUI** — Browse files, configure tracks, and monitor encoding progress in the terminal
- **Two operating modes** — Re-encode to AV1, or demux/remux to repackage and strip tracks without recompression
- **Hardware acceleration** — Automatically detects and uses NVIDIA NVENC, Intel QSV, or AMD AMF; the choice can be overridden manually in Settings
- **Batch processing** — Convert a single file, a folder, or an entire directory tree recursively
- **Smart preset selection** — Automatically picks encoding parameters based on resolution and HDR type
- **Dolby Vision support** — Keep Dolby Vision in the AV1 output (profile 10) or convert to true HDR10 with static metadata; profile 5 sources are tone-mapped on the GPU
- **Quality presets** — Low, Medium, or High shifts CRF/CQ values across every resolution tier at once; Custom leaves each tier's values manually editable
- **VMAF quality verification** — Scores output quality after encoding; deletes the source only when a VMAF score actually met the threshold (never for remuxes, disabled VMAF, or tone-mapped DV profile 5)
- **Track selection** — Auto-selects audio and subtitle tracks by preferred language; Selects all tracks or first track when no match is found
- **Audio transcoding** — Copy audio tracks untouched (the default) or convert any of them to Opus at the source's own channel layout; per-track in both the TUI and the web UI
- **Daemon mode with web UI** — Run headless and manage the queue from a browser (see [Daemon Mode](#daemon-mode-and-web-ui))
- **Multi-language UI** — Interface available in English (default), Italian, Spanish, French, German, and Chinese; selectable in Settings
- **Configurable** — All key settings adjustable through the built-in configuration screen or `~/.config/av1converter/config.toml`

## Prerequisites

`ffmpeg` and `ffprobe` must be on your `PATH`. **FFmpeg 8.0 or newer is required** for Dolby Vision passthrough ("Dolby Vision profile 10 support in AV1" landed in 8.0; the tool is developed and tested against 8.1).

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

```bash
cargo install av1converter          # from crates.io
nix run github:framicheli/av1converter   # or with Nix (flake)
```

From source:

```bash
git clone https://github.com/framicheli/av1converter.git
cd av1converter
cargo build --release
```

The compiled binary will be at `target/release/av1converter`.

## Usage

```bash
./av1converter
```

No command-line arguments are needed. All interaction happens through the TUI.

```
Usage: av1converter [OPTION]

  (no option)          start the interactive TUI
  --daemon             run the web-UI daemon in the background (must be enabled in Settings)
  --daemon-foreground  run the daemon in the foreground, logging to stdout
  --stop               stop the background daemon
  --status             show whether the daemon is running
  --help               show this help
  --version            show the version
```

### Workflow

1. **Home menu** — Choose to open a single file, a folder, or a folder recursively
2. **File selection** — Navigate with arrow keys; `Space` to toggle, `Enter` to confirm
3. **Track configuration** — Select audio and subtitle tracks to include, and switch the per-file mode (encode or demux/remux) with `r`
4. **File review** — Confirm the queue before encoding starts
5. **Encoding** — Monitor per-file and overall progress; `Esc` to cancel
6. **VMAF verification** — Quality score is computed after each file; source is deleted if the score meets the threshold (encode mode only)
7. **Finish** — View a summary of conversions, skipped files, and space saved

## Modes

Each file in the queue is processed in one of two modes. The mode is chosen per file on the track configuration screen and can be toggled with `r`.

### Encode Mode (Encode Video → AV1)

The default mode for non-AV1 sources. The video stream is re-encoded to AV1 using the detected hardware or software encoder, with parameters chosen automatically from the resolution and HDR format (see [Encoding Presets](#encoding-presets)). Selected subtitle tracks are copied into the output, as are audio tracks unless you convert them to Opus (see [Audio Transcoding](#audio-transcoding)). Output is written to the configured container (e.g. `mkv`) with the configured suffix (default `_av1`), and the result is verified with VMAF before the source can be deleted.

### Demux/Remux Mode (Remux Only → Copy Video)

A fast, lossless repackaging mode that copies the video, audio, and subtitle streams without recompression. Use it to change the container, drop unwanted audio/subtitle tracks, or clean up files that are already AV1 — no quality is lost and the operation is near-instant since nothing is re-encoded. The output keeps the source file's container extension and uses the `_remux` suffix. Because no video encoding happens, VMAF verification is skipped.

Audio is the one exception: tracks you convert to Opus are re-encoded even here, so an already-AV1 file with an oversized lossless track can be shrunk without touching the video.

Files that are already encoded in AV1 default to demux/remux mode automatically; everything else defaults to encode mode.

### Dolby Vision Handling

When a Dolby Vision source is queued for encoding, a dialog asks how to convert it (reopen it anytime with `d`):

1. **AV1 with Dolby Vision (profile 10)** — the DV dynamic metadata (RPU) is carried into the AV1 stream. Requires the SVT-AV1 encoder; hardware encoders (NVENC/QSV/AMF) cannot write the RPU and always produce HDR10. For cross-compatible profiles (7/8) the HDR10 base layer is preserved, so players without DV support still get correct HDR10 playback.
2. **AV1 with true HDR10** — the DV layer is dropped and the HDR10 static metadata (mastering display, MaxCLL/MaxFALL) from the source is written into the AV1 stream.

**Profile 5** sources (IPT-PQ-c2, no HDR10-compatible base layer) are special: converting to HDR10 tone-maps the video on the GPU via `libplacebo` (requires Vulkan), while keeping DV produces output that only plays correctly on DV-capable players. HDR10 conversion is the recommended default for profile 5; keeping DV is recommended for profiles 7/8. VMAF verification is skipped for tone-mapped profile 5 output, since the pixels are intentionally changed.

### Audio Transcoding

Audio tracks are copied untouched by default. Any track can instead be converted to Opus, and the choice is per track: a file can keep its lossless track and shrink the commentary, or the other way round.

The channel layout is never changed — a 5.1 track becomes 5.1 Opus, stereo becomes stereo — and the bitrate follows from it, at 64 kbps per channel by default:

| Source track | Opus output |
|--------------|-------------|
| Mono | 64 kbps |
| Stereo | 128 kbps |
| 5.1 | 384 kbps |
| 7.1 | 512 kbps |

Set the allowance per channel with `opus_bitrate_per_channel` (16–256 kbps) to shift the whole table at once. Tracks that are already Opus are left alone rather than re-encoded, unless you turn `skip_already_opus` off.

In the TUI, press `o` on a track in the track configuration screen (`O` applies it to every selected track); the row shows the resulting bitrate, e.g. `[~] 0: eng (DTS 5.1) → OPUS 384k`. In the web UI, use the **Tracks** button on any queued job that has not started encoding yet. `audio.default_mode` sets what newly queued files start out as, which is what the daemon uses when nobody configures a job by hand.

Two things worth knowing:

- **Sources are never auto-deleted when audio was transcoded.** `delete_source_on_success` relies on a VMAF score, and VMAF compares video only — it is no evidence that a lossy Opus track is an acceptable replacement for the lossless TrueHD or DTS-HD original. Those jobs keep the source and log why.
- **Object-based audio does not survive.** Converting a TrueHD Atmos or DTS:X track to Opus keeps the channel bed and discards the object metadata. Copy those tracks if you want them intact.

### Keyboard Controls

| Key | Action |
|-----|--------|
| `↑` / `k`, `↓` / `j` | Navigate |
| `Enter` | Select / Confirm |
| `Space` | Toggle file selection |
| `Esc` | Go back / Cancel |
| `Tab` | Switch focus (track config screen) |
| `r` | Switch mode: encode ↔ demux/remux (track config screen) |
| `d` | Change Dolby Vision handling (track config screen, DV sources) |
| `a` | Toggle all audio tracks |
| `s` | Toggle all subtitle tracks |
| `o` | Convert the highlighted audio track to Opus (track config screen) |
| `O` | Convert all selected audio tracks to Opus (track config screen) |
| `h` / `l` | Decrease / Increase config value |
| `s` | Save configuration (config screen) |
| `q` | Quit (with confirmation) |

## Daemon Mode and Web UI

The daemon runs headless with an embedded web UI for managing conversions from a browser: a dashboard with live progress, the queue (add files or whole folders through a server-side file browser, pause, cancel, remove), and a settings page. Track selection and Dolby Vision handling are resolved automatically, using your configured language preferences and encoder; the **Tracks** button on a queued job opens the per-track audio and subtitle choices for that file, up until its encode starts.

Enable it in Settings (or set `enabled = true` under `[daemon]`), then:

```bash
av1converter --daemon      # start in the background
av1converter --status      # is it running, and where
av1converter --stop        # stop it, cancelling any encode cleanly
```

Background mode is Unix-only; elsewhere use `--daemon-foreground`. Logs go to `~/.local/share/av1converter/daemon.log`.

### Security

The web UI can browse the filesystem, start encodes and rewrite the configuration, and there is no authentication by default. It binds to `127.0.0.1` so that only this machine can reach it.

Before exposing it to a network, set both:

- `auth_token` — a shared secret required by every `/api` request. Open the UI once as `http://host:8399/?token=YOUR_TOKEN`; the browser keeps it for the session.
- `browse_root` — the only directory the file browser and the queue will accept paths under.

The daemon prints a warning at startup if it is bound to a non-loopback address with no token set.

## Encoding Presets

Presets are selected automatically based on resolution and HDR format:

| Resolution | HDR          | Preset         | VMAF Model         |
|------------|--------------|----------------|--------------------|
| SD (≤480p) | No           | **SD**         | vmaf_v0.6.1        |
| HD (720p)  | No           | **HD**         | vmaf_v0.6.1        |
| 1080p      | No           | **1080p SDR**  | vmaf_v0.6.1        |
| 1080p      | Yes (HDR10/HLG) | **1080p HDR**  | vmaf_v0.6.1neg     |
| 1080p      | Dolby Vision    | **1080p DV**   | vmaf_v0.6.1neg     |
| 4K         | No              | **4K SDR**     | vmaf_4k_v0.6.1     |
| 4K         | Yes (HDR10/HLG) | **4K HDR**     | vmaf_4k_v0.6.1neg  |
| 4K         | Dolby Vision    | **4K DV**      | vmaf_4k_v0.6.1neg  |

Files already encoded in AV1 default to demux/remux mode instead of being re-encoded.

## Encoder Detection

The tool detects available encoders at startup with the following priority:

1. **NVIDIA NVENC** (`av1_nvenc`) — RTX 40/50 series and compatible Ada/L-series GPUs
2. **Intel Quick Sync** (`av1_qsv`) — Intel Arc GPUs (Linux/Windows only)
3. **AMD AMF** (`av1_amf`) — RDNA3 architecture, RX 7000 series (Linux/Windows only)
4. **SVT-AV1** (`libsvtav1`) — Software fallback; always used on macOS

## Configuration

Configuration is stored at `~/.config/av1converter/config.toml` and can be edited directly or through the built-in configuration screen.

```toml
language = "en"                # UI language: en, it, es, fr, de, zh (English if omitted)
encoder = "SvtAv1"             # Selected encoder: Nvenc, Qsv, Amf, SvtAv1 (auto-detected on first run, then overridable)
quality_preset = "medium"      # Quality preset: low, medium, high, custom

[Quality]
vmaf_threshold = 90.0          # VMAF score required to consider encoding successful (0–100)
vmaf_enabled = true            # Enable/disable VMAF verification after encoding
delete_source_on_success = false  # Delete source file when VMAF score meets threshold

[Performance]
svt_preset = 4             # SVT-AV1 preset: 0 (slowest) – 13 (fastest)
nvenc_preset = "p4"        # NVENC preset: p1 (best quality) – p7 (fastest)

[Output]
suffix = "_av1"            # Appended to output filenames
container = "mkv"          # Output container (mkv, mp4, …)
same_directory = true      # Write output next to source file
output_directory = null    # Custom output path (used when same_directory = false)

[Tracks]
preferred_audio_languages = ["eng", "ita"]
preferred_subtitle_languages = ["eng"]
select_all_fallback = true # Select all tracks if no preferred language is found

[audio]
default_mode = "copy"          # What newly queued files start as: "copy" or "opus"
opus_bitrate_per_channel = 64  # kbps per channel (16–256); stereo → 128k, 5.1 → 384k
skip_already_opus = true       # Leave tracks that are already Opus alone

[daemon]
enabled = false            # Required before `--daemon` will start
bind_address = "127.0.0.1" # Loopback by default; see the security note below
port = 8399
browse_root = ""           # Confine the web file browser to this directory ("" = whole filesystem)
auth_token = ""            # Shared secret required by /api ("" = no authentication)
```

If `config.toml` cannot be parsed it is kept as `config.toml.bak` and defaults are used for that run, so a typo never costs you your settings.

Each resolution preset also exposes per-encoder quality values (`crf`, `nvenc_cq`, `qsv_quality`, `amf_quality`) and `film_grain` synthesis strength.

`quality_preset` controls how those per-resolution values are managed: `low`, `medium`, and `high` apply built-in CRF/CQ values shifted across every tier at once (overwriting the `presets` table), while `custom` leaves the `presets` table untouched and editable, either directly in the file or via the RF fields on the configuration screen.

`output_directory` can only be set by editing the file directly; the configuration screen exposes `same_directory` but not a path picker.

## Debugging

Set the `AV1_DEBUG` environment variable to enable log output:

```bash
AV1_DEBUG=1 ./av1converter
```

Logs are written to:
- **macOS/Linux:** `~/.local/share/av1converter/av1converter.log`
- **Windows:** `%APPDATA%\av1converter\av1converter.log`
