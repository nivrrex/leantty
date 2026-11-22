# Changelog

## 0.2.11

### Fixed
- Windows: characters the primary font lacks (box drawing, arrows, geometric shapes, decorations,
  braille, math operators, CJK) had no fallback at all — the built-in list was the Linux one
  (`Symbola`, `DejaVu Sans`, `Noto Sans Symbols2`), and on Windows usually only the middle one is
  installed. With no `font.fallback` configured the renderer now seeds a Windows chain —
  `Segoe UI Symbol` → `Segoe UI Emoji` → `Microsoft YaHei` → `MS Gothic` — mirroring the chain the
  Zig implementation has in `src/win32.c`. An explicit `font.fallback` is used verbatim (nothing is
  appended to it).
- A face whose regular style is not named "Regular" (for example DejaVu's "Book") failed to load on
  Windows/DirectWrite; loading now retries with a style description and uses the family's default
  face (the same thing alacritty does).
- Underlines, strikethrough and overline were skipped on blank cells when the rasterizer returned no
  bitmap for the character (DirectWrite returns none for a space), so underlined spaces drew nothing.
- Packaged with the `win` branch merge on top of 0.2.10: the Windows fallback chain above is
  Windows-only; Linux/macOS keep the previous list (`Symbola` → `DejaVu Sans` → `Noto Sans Symbols2`)
  appended after the configured chain, so their behaviour is unchanged.

## 0.2.10

### Fixed
- The GPU `present_ms`/`present_ms_max` counters were always zero: the GPU path pushed a constant 0
  instead of measuring anything, so "present blocked for seconds" was invisible in the perf output
  (it only showed up as a large `render_ms`). Acquiring the swapchain image and presenting are now
  timed separately and reported.
- The window kept rendering (and presenting) while it was occluded. Under a FIFO swapchain the
  compositor stops consuming frames when a window is fully covered, so `get_current_texture` can block
  for a long time and the whole session appears to stall. Occlusion is now tracked and rendering is
  skipped while occluded, with a repaint requested as soon as the window becomes visible again
  (same behaviour as alacritty).

## 0.2.9

### Fixed
- Character fallback ignored `font.fallback`: the glyph-coverage check asked crossfont whether a face
  could render a character, but crossfont silently substitutes another face for missing glyphs and
  reports success. Every character therefore looked "available" in the primary font, the configured
  fallback chain was never used, and the actual face was chosen by fontconfig per character — which is
  why some runs mixed glyphs of different weight/size. Coverage is now queried from each face's real
  cmap (fontdb + ttf-parser), lazily, so an ASCII-only startup pays nothing.

## 0.2.8

### Changed
- The font layer is now **crossfont**, the same library alacritty uses: FreeType + fontconfig on
  Linux, **DirectWrite on Windows** (previously fontdue, which parses whole font files).
  Opening a face is lazy/memory-mapped, so all fonts — including CJK fallbacks — are ready before
  the first frame. The startup flash of "missing glyph" boxes is gone and the background font
  thread (plus its merge/repaint machinery) has been removed.
- Windows now gets colour emoji from DirectWrite instead of monochrome GDI masks.
- Underline, strikethrough and overline geometry is generated once and shared by the CPU and GPU
  renderers, so the two backends stay pixel-identical.

## 0.2.7

### Fixed
- Underline styles are now distinct: `SGR 4:3` (curly) was drawn as a straight line, and dotted/dashed
  used one-pixel spacing. Curly is a real wave, dotted is 1-on/1-off, dashed is 2-on/2-off, double uses
  two separate lines; line thickness and position follow `max(cell_height/16, 1)` like alacritty.
- Emoji and symbol codepoints no longer fall back to "tofu" boxes when the configured fallback chain
  only contains CJK fonts: `Symbola`, `DejaVu Sans` and `Noto Sans Symbols2` are appended to the
  fallback list (missing families are skipped with a notice). Colour emoji fonts are still not used.

## 0.2.6

### Fixed
- Starting from a desktop launcher showed an empty first line: the desktop session has no `TERM`,
  so the shell fell back to a "dumb" terminal and redrew its prompt as spaces after the initial
  window resize, erasing the first row. `TERM` is now defaulted to `xterm-256color` when it is
  missing or `dumb`, like other terminals do.

### Changed
- Cell height is rounded to the nearest pixel instead of rounded up. At 125–150 % scaling the row
  height was one pixel taller than alacritty (13 pt / 150 %: 31 px vs 30 px), which made a 30-row
  window 30 px taller and the text look larger even though the glyphs were the same width.

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
