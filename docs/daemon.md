# Daemon mode and web UI

- [Starting and stopping](#starting-and-stopping)
- [Starting at login](#starting-at-login)
- [The web UI](#the-web-ui)
- [Attaching the TUI](#attaching-the-tui)
- [Logs and queue file](#logs-and-queue-file)
- [Security](#security)

The daemon runs headless with an embedded web UI for managing conversions from a browser. The queue is persisted, so jobs still waiting when the daemon stops are restored on the next start. The daemon refuses to start while a standalone TUI is encoding; finish or quit the TUI first.

## Starting and stopping

Enable it in Settings (or set `enabled = true` under `[daemon]`), then:

```bash
av1converter --start       # start in the background
av1converter --restart     # stop cleanly, then start again
av1converter --status      # is it running, and where
av1converter --stop        # stop it; an active encode starts over on the next start
```

- `--start` returns once the daemon is listening, and reports a failure (a port already in use, for example) instead of claiming success.
- `--stop` and `--restart` stop an active encode cleanly and keep the queue: the file that was encoding starts over, and the jobs behind it stay queued for the next start. `--stop` waits up to 45 seconds.
- `--start`, `--stop` and `--restart` are Unix-only; on Windows run `--start-foreground` and stop it with `Ctrl+C`.
- The old `--daemon` and `--daemon-foreground` spellings remain available as compatibility aliases.

## Starting at login

Turn on **Run at Startup** in Settings, or:

```bash
av1converter --install-service    # systemd user unit (Linux) or launchd agent (macOS)
av1converter --uninstall-service  # remove it
```

`--install-service` also sets `enabled = true`, generates a token if needed, and starts the daemon right away when it is not already running. The unit records your `PATH`, and `XDG_CONFIG_HOME` and `XDG_DATA_HOME` when they are set, at install time, so an FFmpeg installed through Homebrew or another package manager is found and the daemon reads the same `config.toml` and queue as the shell that installed it. Re-run `--install-service` (or turn Run at Startup off and on) after upgrading from an older version, moving FFmpeg, or moving the binary.

- `--stop` lasts until the next login; uninstall is what prevents it coming back.
- On a headless Linux machine the user unit dies at logout unless lingering is enabled (`loginctl enable-linger $USER`).
- Turning **Run at Startup** off from the web settings page leaves the current daemon session running and prevents it from starting at the next login. On macOS the launchd job is also disabled, so a crash does not relaunch it.

## The web UI

The web UI has a dashboard with live progress, the queue and a settings page. From the queue you can add files or whole folders through a server-side file browser, import titles from a disc in the machine's own drive, cancel the encode, the analysis or a rip separately, remove jobs, and clear finished ones. Removing a ripped job, or clearing finished jobs that include one, asks first, because its rip file is deleted.

Track selection and Dolby Vision handling are resolved automatically, using your configured language preferences and encoder. After analysis finishes, a centered dialog opens for the per-file choices; **Apply to remaining files** copies them to the other waiting jobs by track order, while extra tracks keep their automatic defaults and the Dolby Vision choice goes only to files of the same Dolby Vision profile. Closing the dialog without saving stops the automatic prompts until new files are added.

## Attaching the TUI

Running `av1converter` while the daemon is running opens the TUI attached to it instead of starting a separate session: the same screens and keys, showing and changing the daemon's queue, disc runs and settings.

- Quitting with `q` or `Ctrl+C`, or closing the terminal window, closes only the TUI and leaves the encode, the queue and any staged rips running in the daemon.
- When the daemon finishes analysing new files, Track configuration opens as it does in a standalone TUI, as long as you are on the queue. `Esc` there returns to the queue and leaves the jobs waiting in the daemon.
- The Finish screen opens when the daemon's encode ends while you are on the queue, and its **New conversion** clears the daemon's finished jobs as `C` does.
- If the daemon stops answering, the TUI shows `Daemon unreachable, retrying…` and accepts only navigation and quit keys until it is back; other request failures show `The daemon did not answer: …`.
- When the daemon restarts, the TUI closes open dialogs about queue jobs, and on Track configuration discards the selection in progress and returns to the queue.
- If the daemon rejects the TUI's access token, the TUI re-reads `daemon.auth_token` from config.toml once and otherwise shows a token message with the same key limits.
- A TUI from a different release refuses to attach and prints both versions with a hint to run `av1converter --restart`; a daemon restarted into a different release while attached shows the same message and blocks actions.
- A standalone TUI that was already open when the daemon started refuses to encode; restart it to attach.
- When the daemon sees the TUI as a remote client (it listens on a specific non-loopback address, or `behind_proxy` is on), the TUI shows the `[daemon]` and `[disc]` settings and autostart dimmed and read-only, as the web UI does for remote browsers.

## Logs and queue file

- `--start` logs to `$XDG_DATA_HOME/av1converter/daemon.log.<date>` (default `~/.local/share/av1converter/`), rolling daily and keeping the last 7 days. Anything it prints before that log is open lands in `daemon-startup.log` next to it, which is rewritten at every start.
- The macOS launchd agent has no console and writes the same `daemon.log.<date>` and `daemon-startup.log`.
- `--start-foreground` and the systemd unit log to stdout (`journalctl --user -u av1converter`).
- `AV1_DEBUG` raises the log level to debug, as for the TUI (see [Debugging](configuration.md#debugging)).

The queue is saved to `queue.json` in the same directory, with the previous readable save kept as `queue.json.bak`. A `queue.json` that cannot be read is moved to `queue.json.unreadable-<time>` and the queue reloads from `queue.json.bak`; the TUI and the web UI show where the file was kept. A standalone TUI does not persist its queue.

## Security

The web UI can browse the filesystem, start encodes and rewrite the configuration, so it binds to `127.0.0.1` and is guarded by an access token.

### Access token

If `auth_token` is empty or shorter than 32 bytes when the daemon starts, a random 128-bit token is minted and saved to `config.toml`, and `--start` prints the URL that carries it:

```
Web UI listening on http://127.0.0.1:8399/#token=aa2006351b5214a820e3fdc64da870af
```

Open that link once and the browser keeps the token for the tab's session; `av1converter --status` prints it again whenever you need it (use it after `--start-foreground`, which prints the URL without the token). Adding `#token=…` to the address of a page that is already open works too. The fragment after `#` is not sent to the HTTP server or included in HTTP logs. Clearing or weakening `auth_token` does not disable authentication: a strong token is generated on the next start.

The current token is never returned to a browser. A local web session can leave the token field blank to keep it or enter a replacement of at least 32 characters, which becomes the browser session token immediately. The TUI's Settings screen takes the same rule for a replacement; a blank token there is regenerated on the next start.

The token is also what stops a website you visit from reaching the daemon: a page that re-points its own hostname at `127.0.0.1` still cannot produce it. The API also requires JSON for mutations, caps request bodies, and confines restored as well as newly added jobs to `browse_root`.

### Exposing the daemon to a network

- `browse_root` — set it. It is the only directory the file browser and the queue will accept paths under, and it is the difference between "manage my media library" and "read every file this user can read". An empty `browse_root` is refused when the daemon binds outside loopback.
- `bind_address` — leave it on loopback unless you mean it. A non-loopback bind is refused unless `allow_insecure_lan = true` (plain HTTP would otherwise expose the Bearer token on the LAN).

The daemon serves plain HTTP. The built-in server accepts requests before the token is checked, and puts no limit on header size or how long a connection may stay idle. If it must be reachable beyond the local machine, put it behind an HTTPS reverse proxy with connection/request timeouts and rate limiting, and keep the direct daemon port firewalled from untrusted networks. It is intended for trusted local or LAN use, not direct internet exposure.

### Host-level settings

The `[daemon]` and `[disc]` blocks can widen host access or select an executable, so they are writable in the web UI only when it is opened through a loopback origin such as `http://127.0.0.1:8399/` or `http://localhost:8399/`. They remain visible but read-only to LAN and reverse-proxy clients. A request carrying `X-Forwarded-For`, `X-Forwarded-Host`, `X-Real-IP` or `Forwarded` counts as remote. A reverse proxy on the same host that adds none of those headers (nginx's default `proxy_pass`, for example) looks like a loopback browser: set `behind_proxy = true` when one forwards to the web UI, and every web request then counts as remote. `makemkvcon_path` only accepts a file named `makemkvcon` or `makemkvcon64` (with or without `.exe`).
