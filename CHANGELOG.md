# Changelog

## 0.1.1

### Fixed
- Keyboard input in programs that use the kitty keyboard protocol (for example `pi`): Ctrl+C, Ctrl+D, Ctrl+P and other Ctrl combinations did nothing.
- F1-F12, Insert, Delete and Ctrl+[ produced no input on Linux.
- Symbol keys combined with Ctrl (such as Ctrl+Semicolon or Ctrl+Apostrophe) produced no input under the kitty keyboard protocol.

### Changed
- The informational line about the window size being applied asynchronously is no longer printed at startup; it only appears with `LEANTTY_PERF_FRAMES=1`.

## 0.1.0

- Initial release.
