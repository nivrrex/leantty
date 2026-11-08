# W7: V9 a~e scroll semantics probe (record-only) + V8 snapshot cases
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
# keep ZIG_LOG_FILE identical to W4's so cargo does not rerun build.rs
$env:ZIG_LOG_FILE = 'C:\vt\logs\w4-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
$env:GHOSTTY_ZIG_SYSTEM_DIR = "$root\zig-system"
$env:CARGO_TARGET_DIR = "$root\checks\v3_project\target"
Set-Location "$root\checks\v3_project"

& cargo test --test v9_scroll -- --nocapture *>&1 | Tee-Object -FilePath "$root\logs\w7-scroll.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W7 SCROLL FAIL"; exit 1 }
& cargo test --test v8_snapshot -- --nocapture *>&1 | Tee-Object -FilePath "$root\logs\w7-snapshot.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W7 SNAPSHOT FAIL"; exit 1 }
Write-Host "W7 DONE"
