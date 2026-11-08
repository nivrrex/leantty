# W6: KeyEncoder (v7_key.rs) + Ctrl-C through ConPTY (v6_ctrlc.rs)
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
# keep ZIG_LOG_FILE identical to W4's so cargo does not rerun build.rs
$env:ZIG_LOG_FILE = 'C:\vt\logs\w4-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
$env:GHOSTTY_ZIG_SYSTEM_DIR = "$root\zig-system"
$env:CARGO_TARGET_DIR = "$root\checks\v3_project\target"
Set-Location "$root\checks\v3_project"

& cargo test --test v7_key *>&1 | Tee-Object -FilePath "$root\logs\w6-key.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W6 KEYENCODER FAIL"; exit 1 }
& cargo test --test v6_ctrlc -- --nocapture *>&1 | Tee-Object -FilePath "$root\logs\w6-ctrlc-run.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W6 CTRL-C FAIL"; exit 1 }
Write-Host "W6 DONE"
