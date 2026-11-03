# Usage

- [Command line](#command-line)
- [Workflow](#workflow)
- [Modes](#modes)
- [Dolby Vision handling](#dolby-vision-handling)
- [Audio transcoding](#audio-transcoding)
- [Source deletion](#source-deletion)
- [Keyboard controls](#keyboard-controls)
- [Encoding presets](#encoding-presets)
- [Encoder detection](#encoder-detection)

## Command line

With no arguments `av1converter` starts the TUI. The options control the web-UI daemon and maintenance tasks:

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

## Workflow

1. **Home menu** — Open a single file, a folder, or a folder recursively; rip a DVD or Blu-ray; or go to Configuration. Encodes and analysis use the saved configuration; edits on the Configuration screen take effect once saved.
2. **Selection** — Navigate with arrow keys. For a file, `Enter` picks it; for a folder, `Enter` opens it and `Space` selects the highlighted subfolder, or the open folder when the cursor is on `..` or a file. For a disc, pick the drive (skipped when there is only one), or open a disc folder or `.iso` image, then toggle titles with `Space`; extraction feeds the same steps below.
3. **File review** — Confirm the list of files found (multiple files or a folder only; a single file skips this step).
4. **Analysis** — Each file is probed for its streams, resolution and HDR format.
5. **Track configuration** — Select audio and subtitle tracks to include (see [Subtitle files](#subtitle-files)), and switch the per-file mode (encode or demux/remux) with `r`. Encoding starts as soon as the first file is confirmed; the others can be configured while it runs, or all at once with `A`.
6. **Encoding and VMAF verification** — Monitor per-file and overall progress; `a` adds more files, folders or a disc while work runs, and `Esc` asks before cancelling; when a rip, analysis and encode run together, `Esc` cancels the rip, `A` the analysis and `E` the encode. Cancelling during analysis stops only the analysis; files already analysed stay queued. When analysis or encoding finishes, the TUI moves on to Track configuration or Finish only from the Queue, Track configuration or Finish screens; any other screen stays open. Mean and minimum sampled-frame VMAF scores are computed after each file, and the source may then be deleted (see [Source deletion](#source-deletion)).
7. **Finish** — View a summary of conversions, skipped files, and space saved; `Enter` starts a new conversion after a confirmation, `Esc` returns to the queue.

### Subtitle files

Subtitle files next to the source are listed as extra subtitle tracks, titled with their file name:

- Files named `<name>.<ext>`, `<name>.<lang>.<ext>` or `<name>.<lang>.forced.<ext>`.
- When the source is the only video in its folder, every subtitle file there, such as `English.srt`, `ara.srt` or `Brazilian.por.srt`, with the language taken from the last code or English language name in the file name. `.txt` files are left out of this case.

`<ext>` is `srt`, `ass`, `ssa`, `vtt`, `sup`, `idx` (with its `.sub`), `sub` (MicroDVD), `smi`, `sami` or `txt`. A file with no subtitle stream in it is ignored, and subtitle files are never deleted.

## Modes

Each file in the queue is processed in one of two modes, chosen per file on the track configuration screen and toggled with `r`. Files already encoded in AV1 default to demux/remux; everything else defaults to encode.

### Encode (video → AV1)

The video stream is re-encoded to AV1 using the detected hardware or software encoder, with parameters chosen from the resolution and HDR format (see [Encoding presets](#encoding-presets)). Selected subtitle tracks are copied when the output container supports their format, otherwise compatible text subtitles are converted; audio tracks are copied unless you convert them to Opus. Output is written to the configured container (`mkv`, `mp4` or `webm`) with the configured suffix (default `_av1`), and the result is verified with VMAF before the source can be deleted. MKV output keeps attachments such as embedded fonts; cover art is not carried into any container, so a source with cover art is kept. Subtitles the container cannot hold (bitmap subtitles in WebM, bitmap subtitles other than DVD in MP4) are left out, and WebM output converts any audio that is not already Opus or Vorbis to Opus.

### Demux/remux (copy video)

A fast, lossless repackaging mode that copies the video, audio, and subtitle streams without recompression. Use it to drop unwanted audio/subtitle tracks or clean up files that are already AV1. The output keeps the source file's container extension and uses the `_remux` suffix. VMAF verification is skipped.

Audio is the one exception: tracks you convert to Opus are re-encoded even here, so an already-AV1 file with an oversized lossless track can be shrunk without touching the video.

## Cropping and deinterlacing

DVDs are often interlaced or telecined, and many films carry black bars. The `[video]` settings (the **Picture** section of both settings screens) handle both. Every option is off by default.

- **Crop black bars** (`crop`) decodes a short stretch at eight points across the video with FFmpeg's `cropdetect` and crops to the area that any of them showed as picture. A dark scene therefore never loses picture another sample saw. No crop is applied when fewer than half the samples show a picture, or when the crop would keep less than a quarter of the width or height.
- **Deinterlace** (`deinterlace`) samples the same points with `idet`. When at least a fifth of the frames are interlaced, the video is deinterlaced with `bwdif`, one output frame per source frame, in the field order found. A stream ffprobe reports as progressive is never deinterlaced.
- **Inverse telecine** (`inverse_telecine`, needs `deinterlace`) treats video with the repeated fields of 3:2 pulldown as telecined film. Instead of deinterlacing it, `fieldmatch` and `decimate` restore the original 23.976 fps frames.
- **Crop/deinterlace disc rips only** (`disc_rips_only`) applies all of the above only to titles ripped from a disc.

Detection runs when each encode starts. Its result, such as "cropped to 720×432, deinterlaced" or "inverse telecined", is shown on the job's queue row in the TUI and the web UI, and on the TUI Finish screen. Remuxes, encodes that keep Dolby Vision, and tone-mapped profile 5 encodes are never cropped or deinterlaced. If detection fails, the file is encoded unchanged and the log says why.

VMAF scores the output against the source cropped and deinterlaced in exactly the same way. After inverse telecine, frames are paired by position, since the dropped frames change the timestamps. A passing score shows that the encode matches the processed picture. It cannot show that the crop or deinterlace itself was right, so such a source is kept unless `delete_processed_source` is on (see [Source deletion](#source-deletion)).

## Dolby Vision handling

When a Dolby Vision source is queued for encoding with SVT-AV1, a dialog asks how to convert it (reopen it anytime with `d`). Hardware encoders (NVENC, QSV, AMF) cannot write the RPU, so with them the output is HDR10 without asking.

1. **AV1 with Dolby Vision (profile 10)** — the DV dynamic metadata (RPU) is carried into the AV1 stream. For cross-compatible profiles (7/8) the HDR10 base layer is preserved, so players without DV support still get correct HDR10 playback.
2. **AV1 with true HDR10** — the DV layer is dropped and the HDR10 static metadata (mastering display, MaxCLL/MaxFALL) from the source is written into the AV1 stream when encoding with SVT-AV1. Hardware encoders still convert to PQ HDR10 color tags, but do not currently re-attach mastering/CLL SEI.

**Profile 5** sources (IPT-PQ-c2, no HDR10-compatible base layer) are special: converting to HDR10 tone-maps the video on the GPU via `libplacebo` (requires Vulkan), while keeping DV produces output that only plays correctly on DV-capable players. HDR10 conversion is the recommended default for profile 5; keeping DV is recommended for profiles 7/8. VMAF verification is skipped for tone-mapped profile 5 output, since the pixels are intentionally changed. Sources that carry Dolby Vision side data without a readable `dv_profile` are rejected at analysis so a missing profile cannot skip tonemap and produce wrong colors.

HDR10+ dynamic metadata is not preserved; only static HDR10 mastering/CLL (on SVT-AV1) and PQ/HLG color tags are carried forward.

## Audio transcoding

Audio tracks are copied untouched by default. Any track can instead be converted to Opus, per track: a file can keep its lossless track and shrink the commentary, or the other way round.

The channel count and order are preserved (uncommon layouts use independent Opus mapping instead of being silently downmixed), and the bitrate follows the channel count, at 64 kbps per channel by default:

| Source track | Opus output |
|--------------|-------------|
| Mono | 64 kbps |
| Stereo | 128 kbps |
| 5.1 | 384 kbps |
| 7.1 | 512 kbps |

Set the allowance per channel with `opus_bitrate_per_channel` (16–256 kbps) to shift the whole table at once. Tracks that are already Opus are left alone rather than re-encoded, unless you turn `skip_already_opus` off.

In the TUI, press `o` on a track in the track configuration screen (`O` applies it to every selected track); the row shows the resulting bitrate, e.g. `[~] 0: eng (DTS 5.1) → OPUS 384k`. In the web UI, a centered track configuration dialog opens automatically after analysis; the **Tracks** button reopens it for any queued job that has not started encoding yet. `audio.default_mode` sets what newly queued files start out as, which is what the daemon uses when nobody configures a job by hand.

**Object-based audio does not survive.** Converting a TrueHD Atmos or DTS:X track to Opus keeps the channel bed and discards the object metadata. Copy those tracks if you want them intact.

## Source deletion

With `delete_source_on_success` on, the source is deleted only when the job ran in encode mode, was not cancelled, and both the mean and the minimum sampled-frame VMAF score meet `vmaf_threshold`.

VMAF compares video only, so the source is also kept whenever something besides the video changes. That covers jobs that:

- transcode audio, including WebM output converting audio that is not Opus or Vorbis;
- lose Dolby Vision that was set to be kept (hardware encoders cannot write it and convert to HDR10);
- convert or leave out a selected subtitle (bitmap subtitles in MP4/WebM, ASS styling turned into `mov_text` or WebVTT);
- drop cover art or attachments (every container except MKV drops attachments; cover art is dropped everywhere);
- drop streams that cannot be selected (a second video stream, data streams such as timecode or GPS tracks, a Dolby Vision profile 7 enhancement layer);
- reduce the source's chroma or bit depth (everything is encoded as 4:2:0 10-bit, so 4:2:2, 4:4:4, RGB, alpha and 12-bit sources lose detail VMAF does not measure);
- crop or deinterlace the picture, unless `delete_processed_source` is on (see [Cropping and deinterlacing](#cropping-and-deinterlacing)).

A kept source says why: in the queue and Finish screens of the TUI, on the queue row in the web UI, and in the log.

**Every deletion decision is written down.** With `delete_source_on_success` on, each VMAF-verified job appends one JSON line to `$XDG_DATA_HOME/av1converter/deletions.jsonl` (default `~/.local/share/av1converter/deletions.jsonl`), holding the time (seconds since the Unix epoch), the source and output paths and sizes, the mean and minimum VMAF scores, the threshold, whether the source was `deleted` or `kept`, and the reason it was kept. The line is written and flushed before the source is removed: when the ledger cannot be written, the source is kept.

## Keyboard controls

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
| `b` | Configuration | Pick the output, browse-root or staging folder in the file explorer; the path opens for editing, and `Enter` commits it |
| `s` | Configuration | Save configuration |
| `1` / `2` | Dolby Vision dialog | Pick an option; `↑`/`↓` or `Tab` then `Enter` also work. `Esc` keeps the current choice, or applies the recommended one when there is none yet |
| `y` / `n` | Confirmation dialogs | Answer yes / no |
| `←` / `h`, `→` / `l`, `Enter` | Confirmation dialogs | Highlight Yes or No, then answer with the highlighted one (No unless changed) |
| `q`, `Ctrl+C` | Everywhere | Quit (with confirmation, which mentions unsaved configuration changes; `q` is ignored while editing a text field) |

## Encoding presets

Presets are selected automatically based on resolution and HDR format. Classification uses the short and long sides (so portrait 1080×1920 matches landscape 1920×1080):

- **SD** — below HD (e.g. 854×480)
- **HD** — long ≥ 1280 or short ≥ 720, and not yet Full HD (e.g. 1280×720, 1280×800, 1366×768)
- **Full HD** — long ≥ 1920 or short ≥ 1080, up to (but not including) UHD; **1440p / QHD uses the 1080p preset matrix**
- **UHD** — long ≥ 3000 or short ≥ 1800
- **Above 4K** — long ≥ 4097 or short ≥ 2161 (uses the 4K presets)

| Resolution | HDR | Preset | VMAF Model |
|------------|-----|--------|------------|
| SD (e.g. 480p/576p) | Any | **SD** | vmaf_v0.6.1 (HDR: vmaf_v0.6.1neg) |
| HD (720p, laptop 16:10, …) | Any | **HD** | vmaf_v0.6.1 (HDR: vmaf_v0.6.1neg) |
| 1080p / 1440p | No | **1080p SDR** | vmaf_v0.6.1 |
| 1080p / 1440p | Yes (HDR10/HLG) | **1080p HDR** | vmaf_v0.6.1neg |
| 1080p / 1440p | Dolby Vision | **1080p DV** | vmaf_v0.6.1neg |
| 4K | No | **4K SDR** | vmaf_4k_v0.6.1 |
| 4K | Yes (HDR10/HLG) | **4K HDR** | vmaf_4k_v0.6.1neg |
| 4K | Dolby Vision | **4K DV** | vmaf_4k_v0.6.1neg |

The 4K VMAF models are used when the longer side is at least 3840 pixels (portrait 2160×3840 included); narrower 4K-tier sources use the 1080p models. Dolby Vision counts as HDR for model selection. VMAF scores every 10th frame (`n_subsample=10`).

### Automatic CRF

**CRF selection** in the settings (`crf_mode` in the configuration file) chooses how each encode's CRF is picked. **Fixed**, the default, uses the presets table above. **Automatic** searches for it before the encode:

1. Short samples of the source are taken: the whole file when it is 30 seconds or shorter, otherwise one 10-second sample per ten minutes of running time, between two and five, spread evenly.
2. The samples are encoded at a candidate value and scored with VMAF against the same stretch of the source, cropped and deinterlaced like the encode. A value passes when the samples' mean score is at least one point above `vmaf_threshold` and their worst frame meets it.
3. The search starts at the presets-table value and bisects up to 12 values either side of it, keeping the highest value that passes, which gives the smallest file that still meets the threshold. It takes about five rounds of sample encodes.
4. The whole file is encoded at that value and verified as usual. If the full output still misses the threshold, it is removed and encoded once more, 4 lower, and that result is verified and used. When no value in the range passed on the samples, the file is encoded once at the lowest value tried and not redone.

The queue shows **Finding CRF...** with its progress during the search, and the CRF each encode uses appears on the queue row, on the Finish screen and in the web UI. A search that cannot run (for example when the source duration is unknown) falls back to the presets-table value. Automatic mode needs VMAF; with VMAF off, and for remuxes and tone-mapped Dolby Vision profile 5 encodes, the presets table is used.

## Encoder detection

On first run (when no `config.toml` exists yet) the tool encodes one test frame with each hardware AV1 encoder, in this order, and picks the first that succeeds. A test encode that takes longer than 10 seconds counts as a failure. Change the encoder in Settings afterwards:

1. **NVIDIA NVENC** (`av1_nvenc`), e.g. RTX 40/50 series
2. **Intel Quick Sync** (`av1_qsv`), e.g. Arc GPUs
3. **AMD AMF** (`av1_amf`), e.g. RX 7000 series
4. **SVT-AV1** (`libsvtav1`), the software fallback when no hardware encoder works (always on macOS)
