# Disc ripping

Titles can be imported straight from a DVD or Blu-ray: pick **Rip DVD / Blu-ray** on the home menu, or **+ Disc** in the web UI. Selected titles are extracted one at a time to a staging directory, then analyzed, track-configured and encoded exactly like a file you opened yourself. Ripping and encoding overlap (the next title reads from the disc while the previous one encodes), so peak disk use stays at one rip plus one encode.

Besides a physical drive, a disc folder (`VIDEO_TS` / `BDMV`) or an `.iso` image can be opened. Titles shorter than 60 seconds are hidden, and output is named `<disc label>_t<NN>` plus the configured suffix.

## MakeMKV

Ripping needs **MakeMKV**, which is not bundled:

| Platform | What to install | Where `makemkvcon` ends up |
|---|---|---|
| macOS | The `.dmg` from [makemkv.com](https://www.makemkv.com/) | Inside the app bundle, at `/Applications/MakeMKV.app/Contents/MacOS/makemkvcon`. The GUI never has to be opened. |
| Linux | `makemkv-oss` + `makemkv-bin` from source, or a distro package (AUR `makemkv`, the Ubuntu PPA) | On `PATH`. The GUI is optional: `./configure --disable-gui` skips Qt entirely. Your user must be in the `cdrom` group. |
| Windows | The installer from makemkv.com | `C:\Program Files (x86)\MakeMKV\`, or `C:\Program Files\MakeMKV\` (both are searched) |

Set `makemkvcon_path` under `[disc]` if it lives somewhere else. A Flatpak install, for instance, needs a small wrapper script since it is run through `flatpak run`. The path must name a file called `makemkvcon` or `makemkvcon64` (with or without `.exe`), so name the wrapper `makemkvcon`.

DVD decryption is free permanently. Blu-ray needs a purchased MakeMKV licence or the free beta key, and **that key expires every couple of months**: refresh it in MakeMKV when the app reports it as expired. That expiry is MakeMKV's, not this tool's.

## Staging and output

- An existing output directory must be configured before a rip can start. A disc job cannot write next to its source (the staging directory), so disc jobs write there even when `same_directory` is on.
- Set `staging_directory` to a scratch drive: a Blu-ray title needs 100 GB or more.
- The staging file is deleted once its encode succeeds. It is kept when the encode fails, scores under the VMAF threshold, or its VMAF check cannot run. With `vmaf_enabled = false` nothing judges the output, so a finished encode deletes the rip: leave verification on for a disc you cannot rip again.
- Staging directories left behind by an interrupted rip are removed when the TUI or the daemon starts and whenever a disc run ends. Only the folders AV1Converter created for a rip are removed; anything else in the staging directory is left alone.

## Known limitation: Dolby Vision profile 7 (UHD Blu-ray)

MakeMKV extracts profile 7 discs with the Dolby Vision enhancement layer as a *separate* MKV track, and FFmpeg will not recombine it. Those discs encode from the HDR10 base layer, and the EL/RPU track shows up as a stray stream on the track screen. This is a MakeMKV/FFmpeg limitation, not a bug in the conversion.
