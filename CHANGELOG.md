# Changelog

## 0.2.0

### Added
- `-e/--command <PROGRAM> [ARGS...]` runs a program instead of the configured shell; everything after
  it is passed to that program verbatim (the window closes when it exits).
- `--working-directory <DIR>` starts the session in the given directory (the program inherits it).

### Changed
- The default working directory is now the directory leantty was started from, instead of `/tmp`.
- Startup no longer prints a notice about font hinting being unsupported; the notice only appears if
  `hinting` is explicitly enabled.

## 0.1.1

### Fixed
- Keyboard input in programs that use the kitty keyboard protocol (for example `pi`): Ctrl+C, Ctrl+D, Ctrl+P and other Ctrl combinations did nothing.
- F1-F12, Insert, Delete and Ctrl+[ produced no input on Linux.
- PageUp / PageDown now reach the program; keyboard scrolling is on Shift+PageUp / Shift+PageDown.
- Symbol keys combined with Ctrl (such as Ctrl+Semicolon or Ctrl+Apostrophe) produced no input under the kitty keyboard protocol.

### Changed
- The informational line about the window size being applied asynchronously is no longer printed at startup; it only appears with `LEANTTY_PERF_FRAMES=1`.

## 0.1.0

- Initial release.
