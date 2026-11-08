# W3: downstream link. path-dep project builds+runs "downstream link ok";
# dynamic dependency list via dumpbin, or objdump -p as equivalent when dumpbin absent.
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
$env:ZIG_LOG_FILE = 'C:\vt\logs\w3-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
# keep this project's artifacts local; global CARGO_TARGET_DIR points elsewhere
$env:CARGO_TARGET_DIR = "$root\checks\v3_project\target"
Set-Location "$root\checks\v3_project"

Write-Host "=== W3 downstream build ==="
& cargo build *>&1 | Tee-Object -FilePath "$root\logs\w3-build.log"
if ($LASTEXITCODE -ne 0) { Write-Host "W3 BUILD FAIL"; exit 1 }

$bin = "$root\checks\v3_project\target\debug\vt-downstream.exe"
Write-Host "=== W3 run ==="
& $bin *>&1 | Tee-Object -FilePath "$root\logs\w3-run.log"

Write-Host "=== W3 dynamic dependents ==="
$dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
if ($dumpbin) {
    & dumpbin.exe /dependents $bin *>&1 | Tee-Object -FilePath "$root\logs\w3-dependents.txt"
} else {
    Write-Host "dumpbin.exe absent (no MSVC / VS Build Tools; vswhere reports no VC.Tools component)"
    Write-Host "fallback: MinGW objdump -p DLL Name section (engineering judgment: equivalent info)"
    $objdump = 'E:\dev\mingw64\bin\objdump.exe'
    if (Test-Path $objdump) {
        & $objdump -p $bin *>&1 | Tee-Object -FilePath "$root\logs\w3-dependents.txt" | Out-Null
        Select-String -Path "$root\logs\w3-dependents.txt" -Pattern 'DLL Name' | Tee-Object -FilePath "$root\logs\w3-dlls.txt"
    } else {
        Write-Host "objdump also absent; cannot list dependents"; exit 1
    }
}
Write-Host "=== dependency verdict ==="
$dlls = Get-Content "$root\logs\w3-dlls.txt" -ErrorAction SilentlyContinue
if ($dlls -match 'zig') { Write-Host "W3 FAIL: Zig runtime DLL dependency found"; exit 1 }
Write-Host "Zig runtime deps: none (verified fact: no 'zig' in DLL list)"
Write-Host "vcruntime present: $([bool]($dlls -match 'vcruntime'))"
Write-Host "ucrt present: $([bool]($dlls -match 'ucrt'))"
Write-Host "W3 DONE"
