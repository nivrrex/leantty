# leantty

A lightweight terminal emulator: VT parsing and terminal state from
[libghostty](https://github.com/ghostty-org/ghostty), rendering with either a CPU rasterizer or
wgpu (Vulkan on Linux, Vulkan/DX12 on Windows, experimental), PTY handling via `portable-pty`,
windowing via `winit`.

## Features

- Correct grid semantics from libghostty: wide characters, combining marks, reflow, scrollback,
  alternate screen, OSC 0/2 titles, OSC 4/104 palette, bracketed paste.
- Two rendering backends with the same output: CPU (software) or GPU (wgpu). Both are verified
  pixel-by-pixel against each other (differences within 1 LSB).
- Selection, copy/paste (X11 PRIMARY/CLIPBOARD, Windows clipboard), scrollback wheel scrolling
  with a visible scrollbar, drag-to-scroll selection that keeps up with output.
- Non-blocking input path: large pastes are written by a dedicated writer thread, so the window
  keeps rendering and stays responsive; an oversized paste is rejected as a whole instead of
  being cut in the middle of an escape sequence.
- Idle means idle: no timers, no periodic repaints when nothing changes.
- TOML configuration, small dependency footprint, no daemon, no systemd.

## Installation

### Debian package (deb)

```sh
sudo apt install ./leantty_*.deb
leantty
```

### Zip (portable)

Unpack the archive anywhere and run `leantty` (`leantty.exe` on Windows). See the notes for
Windows below.

### From source

Requirements: Rust (edition 2024 toolchain), a C toolchain, and the vendored ghostty sources.
The exact environment variables and pinned versions are in [docs/BUILDING.md](docs/BUILDING.md).

```sh
cargo build --release
./target/release/leantty
```

## Quick start

```sh
leantty                              # start with defaults
leantty --config-file my-config.toml # start with a specific configuration file
leantty --help                       # options
```

## Configuration

- Default location: `$XDG_CONFIG_HOME/leantty/leantty.toml` (`~/.config/leantty/leantty.toml`)
  on Linux, `%APPDATA%\leantty\leantty.toml` on Windows.
- Environment variable `LEANTTY_CONFIG` overrides the default location.
- `--config-file <PATH>` overrides both. A path given this way must exist and be readable;
  otherwise leantty prints the error and exits with status 1 without opening a window.
- Templates with every supported key and its default: [config/leantty.linux.toml](config/leantty.linux.toml)
  and [config/leantty.windows.toml](config/leantty.windows.toml). Copy the one for your platform to
  the default location and edit it.

Unknown keys produce a warning on stderr and are ignored; invalid values fall back to the default.

## Command line options

| Option | Description |
|---|---|
| `-h`, `--help` | Print usage and exit (works without a graphical session) |
| `-V`, `--version` | Print the version (`leantty <version>`) and exit |
| `--config-file <PATH>` | Use this configuration file instead of the default |

Invalid usage exits with status 2 and prints a one-line reason plus `Try --help`.

## Keys and mouse

| Action | Default binding |
|---|---|
| Copy selection | `Ctrl+Shift+C`, `Ctrl+Insert`, middle click |
| Paste | `Ctrl+Shift+V`, `Shift+Insert`, right click |
| Scroll back / forward | mouse wheel |
| Select word / line | double click / triple click |
| Extend selection | drag (dragging past the edge keeps scrolling) |
| Cancel a running command | `Ctrl+C` |

Bindings can be changed with `[[keyboard.bindings]]` / `[[mouse.bindings]]` entries in the
configuration file (see the templates).

## Rendering backend

- `renderer.backend = "cpu"` (default): software rasterizer, no GPU driver needed.
- `renderer.backend = "gpu"`: wgpu backend. The GPU path has been tested with Vulkan on AMD and
  NVIDIA hardware; Windows DX12 support is compiled in but **not verified on real hardware**
  (only with the WARP software adapter).
- `renderer.backend = "auto"`: tries the GPU first and falls back to CPU when no usable adapter
  is present (on Windows `auto` always uses the CPU backend).
- Environment variables: `WGPU_BACKEND` (comma separated, e.g. `vulkan`), `WGPU_ADAPTER_NAME`
  (substring match), `LEANTTY_GPU_ALLOW_SOFTWARE=1` (allow a software adapter such as lavapipe).

## Windows notes

- The bundled ConPTY consists of **two files that must be deployed together** (`conpty.dll` and
  `OpenConsole.exe`, next to `leantty.exe`). With only one of them present, leantty falls back to
  the system console and prints a warning.
- Builds are unsigned: Windows SmartScreen will warn on first launch.
- `leantty-portable.cmd` starts the portable build with the configuration file next to it.
- Automatic high-DPI scaling, IME input and clipboard integration work the same way as on Linux,
  but see the known limitations below.

## Known limitations

- **Windows console quirks (system conhost)**: `ESC[7m` may be rewritten into explicit colours,
  `OSC 104` (palette reset) does not reach the application, bracketed-paste markers and SGR mouse
  reports can be swallowed by the console input path, and wrapped lines may be materialised as
  CRLF. The bundled ConPTY improves most of these except `OSC 104`.
- **Ctrl+C after a large paste** discards the bytes that were not written yet, but text that has
  already reached the console input buffer is still executed by the shell.
- **Pastes larger than 12 MiB are rejected as a whole** (nothing is written) to keep the input
  path non-blocking.
- **Windows DX12** is compiled but only tested with the WARP software adapter.
- The CPU and GPU screenshots on Windows differ by about 1% of pixels; the difference is in glyph
  rasterisation, not in layout. This was not observed on Linux.

## Troubleshooting

1. **Black window / nothing renders**: try `renderer.backend = "cpu"`; on Linux check that
   `DISPLAY` or `WAYLAND_DISPLAY` is set.
2. **Chinese/Japanese text shows as boxes**: add a CJK font to `font.fallback` (Linux: e.g.
   `Noto Sans CJK SC`, Windows: `Microsoft YaHei`).
3. **`--config-file` exits with status 1**: the path does not exist or is not readable — the error
   message contains the path.
4. **GPU backend fails to start** (`backend = "gpu"` exits non-zero): no usable Vulkan/DX12
   adapter; use `"auto"` or `"cpu"`.
5. **Paste does nothing in the terminal but works in other apps**: the shell may have bracketed
   paste enabled and the application inside can ignore it — check the program's own handling.
6. **PowerShell 7 crashes at startup inside leantty** with
   `System.ArgumentOutOfRangeException: totalWidth ('-17')` from
   `UpdatesNotification.ShowUpdateNotification`: this happens inside PowerShell's own update
   banner, not in the terminal. Workaround: set `POWERSHELL_UPDATECHECK=Off` in the environment
   before launching the shell. *Limitation (unverified): we could not reproduce the crash on our
   machines — the ConPTY grid size reported to the shell was correct (120x30 matching the
   configured grid, including after resize) and `pwsh` ran normally at widths from 25 to 120
   columns, both inside leantty and in a plain conhost of the same width.*

- **Keyboard scrolling uses Shift+PageUp / Shift+PageDown**; unmodified PageUp / PageDown are
  forwarded to the program. Bindings in a configuration file must use the winit key names
  (`PageUp`, `PageDown`), not the X keysym names (`Page_Up`).
- **Some Ctrl combinations produce no input** (Linux): Ctrl+Semicolon, Ctrl+Comma, Ctrl+Period,
  Ctrl+Apostrophe, Ctrl+Shift+Semicolon, and Ctrl+I / Ctrl+M are not delivered to programs; the
  keysym-to-control-character mapping for these is not provided by the input encoder.
- **Kitty key release events are not sent**: programs that ask for key-up reporting get key presses
  only.

## License

MIT. See [LICENSE](LICENSE); third-party component licenses are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
