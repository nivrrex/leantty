# Changelog

## 0.2.5

### Changed
- `font.size` is now interpreted in **points**, like alacritty: pixels = size × scale × 96/72. Before,
  the value was used directly as pixels, so the same number produced text about 25% smaller; matching
  alacritty now uses the same number in both programs.
- Cell width is rounded to the nearest pixel instead of rounded up, which made every column 1 px wider
  than alacritty (an 80-column window was 1600 px instead of 1520 px at the same font size).

### Fixed
- `Ctrl+Space` sent nothing at all without an input method; it now sends `0x00` like xterm/alacritty.
- `Ctrl+Shift+<letter>` and `Ctrl+Shift+<symbol>` sent nothing or a kitty `CSI u` sequence; on terminals
  that did not enable the kitty keyboard protocol they now send the xterm control byte
  (`Ctrl+Shift+A` → `0x01`, `Ctrl+Shift+[` → `0x1b`), matching alacritty.

## 0.2.4

### Changed
- `Alt+Tab` no longer leaves a Tab. On X11 window managers that grab the chord, the Tab press is
  taken by the WM while its *release* still reaches us, and the WM re-delivers a bare Tab press
  within a few milliseconds after the window regains focus; that replay is now dropped (a bare Tab
  within 100 ms of focus-in), and a Tab arriving just after an Alt release is dropped as well.
- No other functional changes compared with the republished 0.2.3: this version exists so that the release
  tag, the source snapshot and the packages line up (the 0.2.3 tag pointed at the pre-repack source).
  Contents: the IME candidate-window fix and the window-manager shortcut fix described under 0.2.3.

## 0.2.3

### Fixed
- The IME candidate window no longer covers the line being typed and now follows the cursor
  reliably: the anchor is passed in physical pixels (it was scaled twice on HiDPI), and on X11
  (where the input method only accepts a single point instead of a rectangle) it is placed just
  below the cursor cell, with a two-cell-wide area so panels do not push the popup to the window
  corner.
- `Alt+Tab` no longer leaves a Tab (or a shell completion) behind: window-manager shortcuts
  (`Super` combinations and `Alt+Tab`) are no longer forwarded to the program.
- Keys pressed while the window is not focused are ignored, and modifier state is reset when the
  window loses focus, so switching windows while holding Alt no longer makes the next characters
  arrive with an `ESC` prefix.

## 0.2.2

### Fixed
- On startup the shell prompt could keep showing Chinese directory names as boxes and never
  refresh (most visible with the GPU backend); the window now redraws as soon as the fallback
  font finishes loading, and the GPU glyph cache is invalidated at the same time so the real
  glyphs are drawn instead of the "missing glyph" box.

## 0.2.1

### Changed
- Mouse selections are now **streaming (reading order)**: dragging across several lines selects
  the whole line in between, instead of a rectangular block.
- A single click no longer leaves a small highlighted cell behind; it just clears the previous
  selection.

### Fixed
- Copying a multi-line drag that passes over shorter lines no longer drops the middle of the text.

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
