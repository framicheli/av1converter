# Configuration

Configuration is stored at `$XDG_CONFIG_HOME/av1converter/config.toml` (default `~/.config/av1converter/config.toml`). On Windows it is `%APPDATA%\av1converter\config.toml` unless `XDG_CONFIG_HOME` or `HOME` is set (Git Bash and MSYS2 set `HOME`), in which case the paths above apply.

It can be edited directly or through either settings interface. The first run writes a complete file. The excerpt below leaves out the `[presets.*]` tables, so edit the generated file rather than replacing it with this block.

```toml
language = "en"                # UI language: en, it, es, fr, de, zh (English if omitted)
encoder = "SvtAv1"             # Selected encoder: Nvenc, Qsv, Amf, SvtAv1 (auto-detected on first run, then overridable)
quality_preset = "medium"      # Quality preset: low, medium, high, custom (new files get medium; a file without this key loads as custom)

[quality]
vmaf_threshold = 90.0          # Mean and min sampled-frame VMAF must both meet this (0–100; above 0 while delete_source_on_success is on)
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
bind_address = "127.0.0.1" # Loopback by default; see the security notes in docs/daemon.md
port = 8399
browse_root = ""           # Confine the web file browser to this directory ("" = whole filesystem; required when binding off-loopback)
auth_token = ""            # API secret (empty or under 32 bytes = regenerate on next start)
allow_insecure_lan = false # Required to bind off-loopback over plain HTTP
behind_proxy = false       # Treat every web request as remote; set when a reverse proxy on this host forwards to the web UI

[disc]
# makemkvcon_path = ""      # Unset: PATH, then the platform's MakeMKV install location
# staging_directory = ""    # Where ripped titles wait to be encoded (unset = a private per-user folder under the system temp directory)
```

## Notes

**Unreadable file.** If `config.toml` cannot be parsed it is left untouched and defaults are used for that run. Saving settings afterwards first copies the broken file to `config.toml.unreadable-<seconds since the epoch>`, so a second broken file never overwrites the first. `--install-service`, and `--start` when it has to generate an access token, refuse to run until the file is fixed or removed.

**Quality presets.** Each resolution preset exposes per-encoder quality values (`crf`, `nvenc_cq`, `qsv_quality`, `amf_quality`). `film_grain` synthesis strength is SVT-AV1 only. `quality_preset` controls how those values are managed: `low`, `medium`, and `high` apply built-in values across every tier at once (the `[presets.*]` tables are rewritten whenever the configuration is loaded or saved), while `custom` leaves the complete preset matrix editable in either settings interface or the configuration file.

**Output directory.** When `same_directory` is disabled, `output_directory` is required and must be an existing directory; a leading `~` is expanded. A saved directory that has since disappeared does not block saving other settings: the save goes through with a warning. In the web UI, a new or changed `output_directory` must exist and lie inside `browse_root` even while `same_directory` is on, since disc rips always write there; it cannot be cleared while ripped titles are waiting to encode.

**Settings screens.** Every field exists in both the TUI and web settings pages. The TUI hides rows that do not apply (per-tier values outside `custom`, VMAF options while VMAF is off, `film_grain` unless the encoder is SVT-AV1) and the web page greys them out or omits them. Remote browsers see the `[daemon]` and `[disc]` blocks read-only (see [Host-level settings](daemon.md#host-level-settings)). The web Settings tab re-reads the configuration each time it opens, unless it has unsaved edits.

**When changes apply.** Edits take effect once saved. Bind-address and port changes take effect after a daemon restart. Encoder, quality and output changes update waiting jobs; track defaults apply only to files added after the change.

## Debugging

The TUI always writes a log at level `info`, which records every file it encodes, verifies and deletes. Set `AV1_DEBUG` to any value other than `0` or the empty string to raise it to `debug`:

```bash
AV1_DEBUG=1 av1converter
```

Logs roll daily, and the last 7 daily files are kept:

- **macOS/Linux:** `$XDG_DATA_HOME/av1converter/av1converter.log.<date>` (default `~/.local/share/av1converter/`)
- **Windows:** `%LOCALAPPDATA%\av1converter\av1converter.log.<date>`, unless `XDG_DATA_HOME` or `HOME` is set, in which case the macOS/Linux path applies

For the daemon, `AV1_DEBUG` raises the log level of stdout or `daemon.log.<date>` to debug in the same way (see [Logs and queue file](daemon.md#logs-and-queue-file)).
