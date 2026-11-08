# W5 ConPTY: cmd.exe + powershell.exe rounds
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
# keep ZIG_LOG_FILE identical to W4's so cargo does not rerun build.rs (same profile/args anyway)
$env:ZIG_LOG_FILE = 'C:\vt\logs\w4-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
$env:GHOSTTY_ZIG_SYSTEM_DIR = "$root\zig-system"
$env:CARGO_TARGET_DIR = "$root\checks\v3_project\target"
Set-Location "$root\checks\v3_project"

& cargo test --test v5_conpty -- --nocapture --test-threads=1 *>&1 | Tee-Object -FilePath "$root\logs\w5-conpty-run2.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W5 FAIL"; exit 1 }
Write-Host "W5 DONE"
