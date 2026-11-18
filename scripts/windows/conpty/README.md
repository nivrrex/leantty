# ConPTY redistributable: source, version and hashes

This directory ships the **script only**; no binary is committed to the repository.

## Why

Windows 10 LTSC 19044 hosts `conhost`'s ConPTY, which rewrites `ESC[7m` into explicit
`ESC[30m`/`ESC[47m` and materialises soft wraps as CRLF (measured in PHASE3G-WIN / PHASE3K-WIN).
`portable-pty` 0.8.1 already prefers a `conpty.dll` that sits next to the executable and
otherwise falls back to `kernel32` (see `src/win/psuedocon.rs`, `load_conpty`), so shipping
the official DLL pair next to `app.exe` switches the ConPTY implementation without any code
change.

## Source

| item | value |
|---|---|
| NuGet package | `Microsoft.Windows.Console.ConPTY` |
| version | **1.25.261109001** (stable, published 2026-11-11) |
| package sha256 | `3d77f535df848e44225c2a8912ac844fecb23f3057732b639f3034b587496f09` |
| `conpty.dll` (win-x64) | 119 608 bytes, sha256 `e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492` |
| `OpenConsole.exe` (x64) | 1 089 336 bytes, sha256 `b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775` |
| file / product version | 1.25.2611.09001 / 1.25.261109001 |
| Authenticode | both **Valid**, signer `CN=Microsoft Corporation`, timestamped by Microsoft Time-Stamp Service |
| licence | MIT (see `LICENSE-ConPTY.txt`), upstream `https://github.com/microsoft/terminal` |

Inside the package the two files sit in different folders
(`runtimes/win-x64/native/conpty.dll`, `build/native/runtimes/x64/OpenConsole.exe`);
they must be copied **side by side** next to the application.

## Usage

```powershell
pwsh -File scripts/windows/conpty/fetch-conpty.ps1 -OutDir <目录>
# when raw.githubusercontent / nuget CDN needs a proxy:
pwsh -File scripts/windows/conpty/fetch-conpty.ps1 -OutDir <目录> -Proxy <proxy-url>
```

The script verifies the package hash, both file hashes and both Authenticode signatures
before copying anything, and deletes its staging directory on failure.

## Enabling / rolling back

* **Enable**: copy `conpty.dll` and `OpenConsole.exe` next to `app.exe` (same directory).
* **Roll back to the system conhost**: delete or rename those two files.

## Upgrading

1. Look up the new version on NuGet (`api.nuget.org/v3/registration5-semver1/microsoft.windows.console.conpty/index.json`).
2. Download it, then record the new package sha256 and both file sha256 values plus the
   Authenticode result.
3. Update the pinned constants in `fetch-conpty.ps1` and the table above; re-run the
   Windows regression (PHASE3K-WIN t3) before shipping.
