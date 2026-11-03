<div align="center">

<img src="src/daemon/web/favicon.png" alt="AV1Converter logo" width="96">

# AV1Converter

**Batch convert your video library to AV1, and delete the original only when the result is proven good.**

[![CI](https://github.com/framicheli/av1converter/actions/workflows/rust.yml/badge.svg)](https://github.com/framicheli/av1converter/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/av1converter.svg)](https://crates.io/crates/av1converter)
[![Release](https://img.shields.io/github/v/release/framicheli/av1converter)](https://github.com/framicheli/av1converter/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey)

[Quick start](#quick-start) •
[Installation](#installation) •
[Usage](docs/usage.md) •
[Web UI](docs/daemon.md) •
[Disc ripping](docs/disc-ripping.md) •
[Configuration](docs/configuration.md) •
[Changelog](CHANGELOG.md)

</div>

---

AV1Converter is a single Rust binary that drives FFmpeg to re-encode whole folders to AV1. It picks the fastest encoder your machine has, chooses quality settings from each file's resolution and HDR format, scores every result with VMAF, and runs from a **terminal UI** or as a **background daemon with a web UI**.

## Highlights

|                           |                                                                                                                                                 |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| ⚡ **Hardware first**     | Detects NVIDIA NVENC, Intel QSV and AMD AMF, falls back to SVT-AV1 ([details](docs/usage.md#encoder-detection))                                 |
| 🎯 **VMAF-verified**      | The source is deleted only when the output passes VMAF and nothing else was lost ([details](#when-is-the-source-deleted))                       |
| 🌈 **HDR & Dolby Vision** | Keeps Dolby Vision as AV1 profile 10 or converts to HDR10; profile 5 is tone-mapped on the GPU ([details](docs/usage.md#dolby-vision-handling)) |
| 🎚️ **Smart presets**      | Quality picked per resolution and HDR type, with Low, Medium, High or Custom presets ([details](docs/usage.md#encoding-presets))                |
| 🔎 **Automatic CRF**      | Optionally searches each file's CRF on short samples against the VMAF threshold ([details](docs/usage.md#automatic-crf))                        |
| 🔊 **Track control**      | Auto-selects audio and subtitles by preferred language, and picks up subtitle files next to the video                                           |
| 🎵 **Opus audio**         | Copy audio untouched, or convert any track to Opus at its own channel layout ([details](docs/usage.md#audio-transcoding))                       |
| 📦 **Remux mode**         | Repackage and strip tracks without re-encoding the video                                                                                        |
| 💿 **Disc ripping**       | Imports DVD and Blu-ray titles through MakeMKV, ripping the next title while the previous one encodes ([details](docs/disc-ripping.md))         |
| ✂️ **Crop & deinterlace** | Optional black-bar cropping, deinterlacing and inverse telecine, verified by VMAF ([details](docs/usage.md#cropping-and-deinterlacing))         |
| 🌐 **Headless daemon**    | A persistent queue and a token-protected web UI with the same features as the TUI ([details](docs/daemon.md))                                   |
| 🗂️ **Batch anything**     | A single file, a folder, or a whole directory tree recursively                                                                                  |
| 🌍 **Six languages**      | English, Italian, Spanish, French, German and Chinese, in both UIs                                                                              |

## Quick start

```bash
cargo install av1converter     # or brew, nix, AUR, .deb/.rpm: see Installation
av1converter                   # open the terminal UI
```

Prefer a browser? Enable the daemon in Settings, then:

```bash
av1converter --start           # prints the web UI link with its access token
```

> [!IMPORTANT]
> You need `ffmpeg` and `ffprobe` **8.0 or newer** on your `PATH`. See [Prerequisites](#prerequisites) for which FFmpeg features each option needs.

## Prerequisites

`ffmpeg` and `ffprobe` **8.0 or newer** must be on your `PATH` (8.0 added Dolby Vision profile 10 in AV1; the tool is tested against 8.1). The version is not checked at startup. Ripping discs additionally needs [MakeMKV](docs/disc-ripping.md#makemkv).

Not every FFmpeg build includes every feature. What you need depends on what you use:

| FFmpeg capability                      | Needed for                                                                                                      | Required?                                   |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| `libsvtav1`                            | Software AV1 encoding; Dolby Vision passthrough (AV1 profile 10)                                                | Yes, unless you only use hardware encoders  |
| `libvmaf`                              | VMAF quality verification (and source auto-deletion, which depends on it)                                       | Optional                                    |
| `libopus`                              | Converting audio tracks to Opus                                                                                 | Optional — only if you transcode audio      |
| `libplacebo` + a working Vulkan driver | Dolby Vision **profile 5** → HDR10 tone-mapping                                                                 | Optional — only for DV profile 5 conversion |
| `av1_nvenc` / `av1_qsv` / `av1_amf`    | Hardware encoding (needs a matching GPU driver: NVIDIA driver, Intel media driver + libvpl, or AMD AMF runtime) | Optional                                    |

Check what your build supports:

```bash
ffmpeg -version                                  # 8.0+ required for DV passthrough
ffmpeg -h encoder=libsvtav1 | grep dolbyvision   # DV passthrough
ffmpeg -filters | grep libvmaf                   # VMAF verification
ffmpeg -encoders | grep libopus                  # Opus audio transcoding
ffmpeg -filters | grep libplacebo                # DV profile 5 tone-mapping
vulkaninfo --summary                             # Vulkan driver (DV profile 5 only)
```

<details>
<summary><b>Platform notes</b> (Arch, Debian/Ubuntu, Fedora, macOS, Windows)</summary>

- **Arch Linux** — `pacman -S ffmpeg` includes all of the above. For DV profile 5 tone-mapping also install your GPU's Vulkan driver (`vulkan-radeon`, `vulkan-intel`, or `nvidia-utils`).
- **Debian/Ubuntu** — the distro `ffmpeg` includes `libsvtav1`, but is often older than 8.0 (no DV passthrough — selecting "keep Dolby Vision" will fail); `libvmaf` and `libplacebo` also vary by release. Verify with the commands above.
- **Fedora** — use the FFmpeg from RPM Fusion; the freeworld build includes `libsvtav1` and `libvmaf`.
- **macOS** — `brew install ffmpeg` includes `libsvtav1` and `libvmaf` but **not** `libplacebo`/Vulkan: everything works except DV profile 5 → HDR10 tone-mapping (keeping DV still works for profile 5).
- **Windows** — the full builds from [gyan.dev](https://www.gyan.dev/ffmpeg/builds/) or [BtbN](https://github.com/BtbN/FFmpeg-Builds/releases) (GPL variant) include everything.

</details>

## Installation

| Method                         | Command                                                                                                                          |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------- |
| Cargo                          | `cargo install av1converter`                                                                                                     |
| Homebrew (macOS, x86_64 Linux) | `brew tap framicheli/tap && brew install av1converter`                                                                           |
| Nix                            | `nix profile install github:framicheli/av1converter`                                                                             |
| Arch Linux (AUR)               | `git clone https://aur.archlinux.org/av1converter.git && cd av1converter && makepkg -si`                                         |
| Debian / Ubuntu                | `sudo apt install ./av1converter_VERSION_amd64.deb` from the [release page](https://github.com/framicheli/av1converter/releases) |
| Fedora                         | `sudo dnf install ./av1converter-VERSION-1.x86_64.rpm` (needs RPM Fusion)                                                        |
| Binary or source               | Download from the [release page](https://github.com/framicheli/av1converter/releases), or `cargo build --release`                |

See [docs/installation.md](docs/installation.md) for details and for uninstalling (`av1converter --purge` removes configuration and daemon state).

## Usage

Run `av1converter` to open the TUI: pick a file, a folder or a disc, confirm the tracks for each file, and watch the queue encode and verify. Run `av1converter --help` for the daemon and maintenance options.

- [Usage guide](docs/usage.md) — workflow, encode and remux modes, Dolby Vision, audio, key bindings, presets
- [Daemon and web UI](docs/daemon.md) — background service, login autostart, attaching the TUI, security
- [Disc ripping](docs/disc-ripping.md) — MakeMKV setup, staging, known limitations
- [Configuration](docs/configuration.md) — `config.toml` reference, logs and debugging

## When is the source deleted?

Never by default. With `delete_source_on_success` on, a source is deleted only when:

- the file was encoded (not remuxed) and the job was not cancelled,
- both the mean and the minimum sampled-frame VMAF score meet `vmaf_threshold`, and
- nothing besides the video changed: no audio transcoded, no subtitle, attachment, Dolby Vision layer or other stream lost or converted, no chroma or bit depth reduced, and the picture not cropped or deinterlaced (unless `delete_processed_source` is on).

Otherwise the source is kept and both UIs say why. Every decision is recorded in `deletions.jsonl` before anything is removed. The full list of rules is in [Source deletion](docs/usage.md#source-deletion).

---

<div align="center">

Released under the [MIT License](LICENSE) • Built with Rust on top of [FFmpeg](https://ffmpeg.org/), [SVT-AV1](https://gitlab.com/AOMediaCodec/SVT-AV1) and [VMAF](https://github.com/Netflix/vmaf)

</div>
