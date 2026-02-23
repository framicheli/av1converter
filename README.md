# AV1Converter

A terminal-based interactive tool to batch convert video files to the AV1 codec using FFmpeg. It auto-detects available hardware encoders, verifies output quality with VMAF, and manages the full encoding pipeline through a TUI.

## Features

- **Interactive TUI** — Browse files, configure tracks, and monitor encoding progress in the terminal
- **Hardware acceleration** — Automatically detects and uses NVIDIA NVENC, Intel QSV, or AMD AMF
- **Batch processing** — Convert a single file, a folder, or an entire directory tree recursively
- **Smart preset selection** — Automatically picks encoding parameters based on resolution and HDR type
- **VMAF quality verification** — Scores output quality after encoding; deletes source file if the threshold is met
- **Track selection** — Choose which audio and subtitle tracks to include per file
- **Configurable** — All key settings adjustable through the built-in configuration screen or `~/.config/av1converter/config.toml`

## Prerequisites

- `ffmpeg` (with `libsvtav1` and `libvmaf` support)
- `ffprobe`

## Installation

```bash
git clone https://github.com/your-username/av1converter.git
cd av1converter
cargo build --release
```

The compiled binary will be at `target/release/av1converter`.

## Usage

```bash
./av1converter
```

No command-line arguments are needed. All interaction happens through the TUI.

### Workflow

1. **Home menu** — Choose to open a single file, a folder, or a folder recursively
2. **File selection** — Navigate with arrow keys; `Space` to toggle, `Enter` to confirm
3. **Track configuration** — Select audio and subtitle tracks to include
4. **File review** — Confirm the queue before encoding starts
5. **Encoding** — Monitor per-file and overall progress; `Esc` to cancel
6. **VMAF verification** — Quality score is computed after each file; source is deleted if the score meets the threshold
7. **Finish** — View a summary of conversions, skipped files, and space saved

### Keyboard Controls

| Key | Action |
|-----|--------|
| `↑` / `k`, `↓` / `j` | Navigate |
| `Enter` | Select / Confirm |
| `Space` | Toggle file selection |
| `Esc` | Go back / Cancel |
| `Tab` | Switch focus (track config screen) |
| `a` | Toggle all audio tracks |
| `s` | Toggle all subtitle tracks |
| `h` / `l` | Decrease / Increase config value |
| `s` | Save configuration (config screen) |
| `q` | Quit (with confirmation) |

## Encoding Presets

Presets are selected automatically based on resolution and HDR format:

| Resolution | HDR          | Preset         | VMAF Model         |
|------------|--------------|----------------|--------------------|
| SD (≤480p) | No           | **SD**         | vmaf_v0.6.1        |
| HD (720p)  | No           | **HD**         | vmaf_v0.6.1        |
| 1080p      | No           | **1080p SDR**  | vmaf_v0.6.1        |
| 1080p      | Yes          | **1080p HDR**  | vmaf_v0.6.1neg     |
| 1080p      | Dolby Vision | **1080p DV**   | vmaf_v0.6.1neg     |
| 4K         | No           | **4K SDR**     | vmaf_4k_v0.6.1     |
| 4K         | Yes          | **4K HDR**     | vmaf_4k_v0.6.1     |
| 4K         | Dolby Vision | **4K DV**      | vmaf_4k_v0.6.1     |

Files already encoded in AV1 are automatically skipped.

## Encoder Detection

The tool detects available encoders at startup with the following priority:

1. **NVIDIA NVENC** (`av1_nvenc`) — RTX 40/50 series and compatible Ada/L-series GPUs
2. **Intel Quick Sync** (`av1_qsv`) — Intel Arc GPUs (Linux/Windows only)
3. **AMD AMF** (`av1_amf`) — RDNA3 architecture, RX 7000 series (Linux/Windows only)
4. **SVT-AV1** (`libsvtav1`) — Software fallback; always used on macOS

## Configuration

Configuration is stored at `~/.config/av1converter/config.toml` and can be edited directly or through the built-in configuration screen.

```toml
[Quality]
vmaf_threshold = 90.0      # VMAF score required to delete the source file (0–100)
vmaf_enabled = true        # Enable/disable VMAF verification

[Performance]
svt_preset = 4             # SVT-AV1 preset: 0 (slowest) – 13 (fastest)
nvenc_preset = "p7"        # NVENC preset: p1 (best quality) – p7 (fastest)

[Output]
suffix = "_av1"            # Appended to output filenames
container = "mkv"          # Output container (mkv, mp4, …)
same_directory = true      # Write output next to source file
output_directory = null    # Custom output path (used when same_directory = false)

[Tracks]
preferred_audio_languages = ["eng", "ita"]
preferred_subtitle_languages = ["eng"]
select_all_fallback = true # Select all tracks if no preferred language is found
```

Each resolution preset also exposes per-encoder quality values (`crf`, `nvenc_cq`, `qsv_quality`, `amf_quality`) and `film_grain` synthesis strength.

## Debugging

Set the `AV1_DEBUG` environment variable to enable log output:

```bash
AV1_DEBUG=1 ./av1converter
```

Logs are written to:
- **macOS/Linux:** `~/.local/share/av1converter/av1converter.log`
- **Windows:** `%APPDATA%\av1converter\av1converter.log`
