# W8release cargo build --release + Zig  + V8  + 100MB 
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
$env:ZIG_LOG_FILE = 'C:\vt\logs\w8-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"

Write-Host "=== W8 workspace release  ==="
Set-Location "$root\vendor\libghostty-rs"
& cargo build --release *>&1 | Tee-Object -FilePath "$root\logs\w8-release-build.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W8 RELEASE BUILD FAIL"; exit 1 }

Write-Host "=== W8 zig ==="
Get-Content $env:ZIG_LOG_FILE | Tee-Object -FilePath "$root\logs\w8-zig-invocations.txt"

Write-Host "=== W8  ==="
$libs = Get-ChildItem "$env:CARGO_TARGET_DIR\release\build\libghostty-vt-sys-*\out\ghostty-install\lib\*" -File
$libs | ForEach-Object { Write-Host ("{0} {1} bytes" -f $_.Name, $_.Length) } | Tee-Object -FilePath "$root\logs\w8-release-lib.txt"

Write-Host "=== W8 V8 release, 30 ==="
Set-Location "$root\checks\v3_project"
& cargo test --release --test v8_snapshot -- --nocapture *>&1 | Tee-Object -FilePath "$root\logs\w8-snapshot.log"

Write-Host "=== W8 100MB release==="
& cargo test --release --test w8_throughput -- --nocapture *>&1 | Tee-Object -FilePath "$root\logs\w8-throughput.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W8 TEST FAIL"; exit 1 }
Write-Host "W8 DONE"
