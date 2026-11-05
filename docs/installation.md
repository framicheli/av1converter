# Installation

`ffmpeg` and `ffprobe` 8.0 or newer must be on your `PATH` whichever way you install; see [Prerequisites](../README.md#prerequisites) for the FFmpeg features each option needs.

## Cargo

```bash
cargo install av1converter
```

## Nix

The flake builds the binary only; `ffmpeg` and `ffprobe` still have to be on your `PATH`.

```bash
nix run github:framicheli/av1converter             # run without installing
nix profile install github:framicheli/av1converter # install into your Nix profile
```

## Homebrew

macOS (Apple Silicon and Intel) and x86_64 Linux; installs the prebuilt release binary and pulls in `ffmpeg`.

```bash
brew tap framicheli/tap
brew install av1converter
```

The formula lives in [framicheli/homebrew-tap](https://github.com/framicheli/homebrew-tap) and is updated automatically by the release workflow.

## Arch Linux (AUR)

After the package is published to the AUR:

```bash
git clone https://aur.archlinux.org/av1converter.git
cd av1converter
makepkg -si
```

## Debian, Ubuntu, and Fedora

Download the package for your release from the [release page](https://github.com/framicheli/av1converter/releases), then install it:

```bash
sudo apt install ./av1converter_VERSION_amd64.deb
sudo dnf install ./av1converter-VERSION-1.x86_64.rpm
```

On Fedora the RPM depends on the `ffmpeg` package from RPM Fusion; enable RPM Fusion first (see the platform notes under [Prerequisites](../README.md#prerequisites)).

## Release binary or source

Release archives are checksummed in `SHA256SUMS`. Extract the target-suffixed executable, rename it to `av1converter` (or `av1converter.exe` on Windows), and put it somewhere on your `PATH`.

To build from source:

```bash
git clone https://github.com/framicheli/av1converter.git
cd av1converter
cargo build --release
```

The compiled binary will be at `target/release/av1converter`.

## Uninstallation

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
