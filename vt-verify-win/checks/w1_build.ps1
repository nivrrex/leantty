# W1: native Windows build. Records Rust target, linker, zig target params
# (msvc vs gnu) from build.rs, /MD vs /MT handling, static lib artifacts,
# header list, and any DLL output.
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
$env:ZIG_LOG_FILE = 'C:\vt\logs\w1-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
Remove-Item $env:ZIG_LOG_FILE -ErrorAction SilentlyContinue

Write-Host "=== W1a Rust target / linker ==="
rustc -vV | Tee-Object -FilePath "$root\logs\w1-rustc.txt"
Write-Host "CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR"
Write-Host "default linker probe (rustc -C link-arg=-Wl,--version):"
Set-Content -Path "$root\logs\w1-linker-probe.rs" -Value 'fn main(){}'
& rustc -C link-arg=-Wl,--version "$root\logs\w1-linker-probe.rs" -o "$root\logs\w1-linker-probe.exe" *>&1 |
    Select-String -Pattern 'GNU ld|lld|Microsoft|collect2|ld\.exe' |
    Tee-Object -FilePath "$root\logs\w1-linker.txt"

Write-Host "=== W1 build (cargo clean -p first to force build.rs rerun) ==="
Set-Location "$root\vendor\libghostty-rs"
& cargo clean -p libghostty-vt-sys *>&1 | Out-Null
& cargo build --workspace -v *>&1 | Tee-Object -FilePath "$root\logs\w1-build.log"
$rc = $LASTEXITCODE
if ($rc -ne 0) {
    Write-Host "W1 BUILD FAIL exit=$rc"
    Write-Host "NOTE: default path (no GHOSTTY_SOURCE_DIR) makes build.rs git clone github.com;"
    Write-Host "github.com is unreachable on this host; first attempt failed at build.rs git clone exit 128."
    Write-Host "Evidence: logs/w1-build-default.log"
    exit 1
}

Write-Host "=== W1a zig invocations (via ZIG wrapper log) ==="
Get-Content $env:ZIG_LOG_FILE -ErrorAction SilentlyContinue | Tee-Object -FilePath "$root\logs\w1-zig-invocations.txt"
$inv = Get-Content $env:ZIG_LOG_FILE | Where-Object { $_ -match 'zig build' } | Select-Object -First 1
$inj = Get-Content $env:ZIG_LOG_FILE | Where-Object { $_ -match 'INJECT' } | Select-Object -First 1
if ($inv -match '-Dtarget=(\S+)') {
    Write-Host "zig target param in build.rs call: $($Matches[1])"
} elseif ($inj -match '-Dtarget=(\S+)') {
    Write-Host "zig target param: $($Matches[1]) (injected by ZIG wrapper; build.rs native path passes none)"
} else {
    Write-Host "no -Dtarget captured"
}
Write-Host "build.rs maps x86_64-pc-windows-gnu -> x86_64-windows-gnu in zig_target();"
Write-Host "native build passes no -Dtarget, so zig defaults to native-native-msvc (see first-attempt error)."

Write-Host "=== W1 /MD vs /MT ==="
$n = (Select-String -Path "$root\vendor\libghostty-rs\crates\libghostty-vt-sys\build.rs" -Pattern '/MD|/MT' -AllMatches | Measure-Object).Count
Write-Host "occurrences of /MD or /MT in build.rs: $n (0 = not distinguished; CRT is zig's call)"

Write-Host "=== W1b artifacts ==="
$out = Get-ChildItem "$env:CARGO_TARGET_DIR\debug\build\libghostty-vt-sys-*\out\ghostty-install" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $out) { Write-Host "ghostty-install dir not found"; exit 1 }
Write-Host "install dir: $($out.FullName)"
Get-ChildItem -Path $out.FullName -Recurse -File | ForEach-Object {
    [PSCustomObject]@{ Rel = $_.FullName.Substring($out.FullName.Length + 1); Bytes = $_.Length }
} | Tee-Object -FilePath "$root\logs\w1-artifacts.txt" | Format-Table -AutoSize
$libs = Get-ChildItem -Path $out.FullName -Recurse -File -Include '*.lib', '*.a', '*.dll', '*.dll.lib'
Write-Host "--- libraries ---"
$libs | ForEach-Object { Write-Host ("{0}  {1} bytes" -f $_.Name, $_.Length) }
$headers = Get-ChildItem -Path (Join-Path $out.FullName 'include') -Recurse -File -Filter '*.h'
Write-Host "header count: $($headers.Count)"
$headers | ForEach-Object { $_.FullName.Substring($out.FullName.Length + 1) } | Tee-Object -FilePath "$root\logs\w1-headers.txt"
Write-Host "W1 DONE"
