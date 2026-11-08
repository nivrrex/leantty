# W4 a~f: run vendor tests/v4_grid.rs as-is on Windows
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
$env:ZIG_LOG_FILE = 'C:\vt\logs\w4-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
$env:GHOSTTY_ZIG_SYSTEM_DIR = "$root\zig-system"
# keep artifacts local; global CARGO_TARGET_DIR points elsewhere
$env:CARGO_TARGET_DIR = "$root\checks\v3_project\target"
Set-Location "$root\checks\v3_project"

& cargo test --test v4_grid -- --nocapture *>&1 | Tee-Object -FilePath "$root\logs\w4-grid.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W4 FAIL"; exit 1 }
Write-Host "W4 DONE"
