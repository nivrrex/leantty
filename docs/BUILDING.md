# Building leantty from source

## Fixed versions

| Component | Version / commit | Where it is pinned |
|---|---|---|
| Rust | edition 2024 toolchain, MSRV 1.96 | `Cargo.toml` (`[workspace.package] rust-version`) |
| Zig | 0.17.1 | required by the ghostty build (`build.zig.zon` `minimum_zig_version` is 0.17.0) |
| ghostty (VT library) | commit `0b8b9984ee560456194262d190aa850224edb1f4` (1.4.1-dev) | `vendor/libghostty-rs/crates/libghostty-vt-sys/build.rs` (`GHOSTTY_COMMIT`) |
| libghostty-rs bindings | 0.2.1 (commit `84373f8d48b8f0b794e259f74a71a52686616f5d`) | `vendor/libghostty-rs` |
| wgpu | `=30.0.2`, `default-features = false` | `crates/render/Cargo.toml` |

## Environment variables used by the build

| Variable | Purpose |
|---|---|
| `ZIG` | path to the `zig` executable (0.17.x). Required — the ghostty build is driven by Zig. |
| `GHOSTTY_SOURCE_DIR` | path to a ghostty source tree at the pinned commit (the bindings compile it). |
| `GHOSTTY_ZIG_SYSTEM_DIR` | path to a pre-populated Zig "system" directory used for the ghostty build. |

These three are also picked up by the project's own gate script; none of them are needed at run
time.

## Steps

1. Install a Rust toolchain that supports edition 2024 (MSRV 1.96).
2. Fetch Zig 0.17.1 from <https://ziglang.org/download/0.17.1/> and verify the SHA-256 published
   in `index.json` for your platform; unpack it anywhere and note the path to `zig`.
3. Provide a ghostty source tree at the pinned commit, and a Zig system directory for it.
4. Export the three variables and build:

   ```sh
   export ZIG=/path/to/zig
   export GHOSTTY_SOURCE_DIR=/path/to/ghostty
   export GHOSTTY_ZIG_SYSTEM_DIR=/path/to/zig-system
   cargo build --release        # binary ends up in target/release/leantty
   ```

5. Optional checks:

   ```sh
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all --check
   ```

## Release artifacts

- Icons and the embedded window icon are generated from `assets/icon/leantty.svg` by
  `sh scripts/gen-icons.sh` (needs `rsvg-convert` or ImageMagick's `convert`). The generated files
  are committed, so this is only needed after changing the SVG.
- Third-party license notices: `sh scripts/release/gen-notices.sh` writes
  `THIRD-PARTY-NOTICES.md` from `cargo metadata` (host plus the Windows target) and stops with a
  non-zero exit code if a dependency is GPL/AGPL-only or has no license information.

## Notes

- The build is offline-capable once the ghostty tree, Zig, and the Cargo registry cache are in
  place; nothing is fetched at run time.
- Windows builds additionally need the two bundled ConPTY files next to the executable; they are
  not part of the build output.
