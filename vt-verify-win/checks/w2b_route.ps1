# W2b: real network cut via default-route removal (firewall rules proven inert here).
# Self-healing: routes restored by scheduled task + finally block even on failure.
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
$env:ZIG_LOG_FILE = 'C:\vt\logs\w2b-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
$env:GHOSTTY_ZIG_SYSTEM_DIR = "$root\zig-system"
Remove-Item $env:ZIG_LOG_FILE -ErrorAction SilentlyContinue

$routes = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue
$routes | Format-List * | Out-File "$root\logs\w2b-routes-before.txt"
Write-Host "default routes before cut:"
$routes | ForEach-Object { "  if=$($_.ifIndex) gw=$($_.NextHop) metric=$($_.RouteMetric) pub=$($_.Publish)" }

$restoreBat = "$root\checks\w2b_restore.bat"
$lines = @('@echo off')
foreach ($r in $routes) {
    $lines += "route add 0.0.0.0 mask 0.0.0.0 $($r.NextHop) metric $($r.RouteMetric) if $($r.ifIndex)"
}
Set-Content -Path $restoreBat -Value ($lines -join "`r`n")
& schtasks /create /tn W2B_RESTORE /tr "$restoreBat" /sc once /st ([DateTime]::Now.AddMinutes(10).ToString('HH:mm')) /f | Out-Null
Write-Host "self-heal task W2B_RESTORE scheduled ($restoreBat contents saved)"

try {
    Write-Host "=== cutting default route(s) ==="
    & route delete 0.0.0.0 | Out-Null
    & route print -4 0.0.0.0 | Select-String '0.0.0.0' | Out-String | Write-Host

    Write-Host "=== block proof A: zig fetch uncached URL (must fail) ==="
    & "$root\toolchain\zig\zig.exe" fetch "https://ziglang.org/download/index.json" 2>&1 | Select-Object -First 6 | Tee-Object -FilePath "$root\logs\w2b-block-zig.log"
    Write-Host "zig fetch exit=$LASTEXITCODE (nonzero = blocked)"

    Write-Host "=== block proof B: cargo search --registry crates-io (must fail) ==="
    Set-Location "$root\vendor\libghostty-rs"
    & cargo search serde --registry crates-io 2>&1 | Select-Object -First 6 | Tee-Object -FilePath "$root\logs\w2b-block-cargo.log"
    Write-Host "cargo search exit=$LASTEXITCODE (nonzero = blocked)"

    Write-Host "=== block proof C: curl (must fail) ==="
    & curl.exe -sS -m 15 -o NUL -w "curl during cut: %{http_code}`n" https://ziglang.org/download/index.json 2>&1 | Tee-Object -FilePath "$root\logs\w2b-block-curl.log"
    Write-Host "curl exit=$LASTEXITCODE (nonzero = blocked)"

    Write-Host "=== W2 offline build under real network cut ==="
    & cargo clean *>&1 | Out-Null
    & cargo build --workspace --offline *>&1 | Tee-Object -FilePath "$root\logs\w2b-offline-build.log"
    $script:rc = $LASTEXITCODE
} finally {
    Write-Host "=== restore routes ==="
    & cmd.exe /c "$restoreBat" | Out-Null
    & schtasks /delete /tn W2B_RESTORE /f 2>&1 | Out-Null
    $now = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue
    $now | Format-List * | Out-File "$root\logs\w2b-routes-after.txt"
    Write-Host "default routes after restore:"
    $now | ForEach-Object { "  if=$($_.ifIndex) gw=$($_.NextHop) metric=$($_.RouteMetric)" }
}
if ($rc -ne 0) { Write-Host "W2B OFFLINE BUILD FAIL exit=$rc"; exit 1 }

Write-Host "=== W2 zig invocation (--system present?) ==="
Get-Content $env:ZIG_LOG_FILE -ErrorAction SilentlyContinue | Tee-Object -FilePath "$root\logs\w2b-zig-invocations.txt"

Write-Host "=== W2c hash dirs: line endings / path length ==="
Write-Host "LongPathsEnabled(registry): $((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem').LongPathsEnabled)"
$names = Get-ChildItem "$root\zig-system" -Directory
Write-Host "zig-system subdir count: $($names.Count)"
$bad = $names | Where-Object { $_.Name -match "[`r`n]" }
Write-Host "dir names containing CR/LF: $($bad.Count) (expect 0)"
$longest = ($names | ForEach-Object { $_.FullName.Length } | Measure-Object -Maximum).Maximum
Write-Host "longest zig-system dir path length: $longest"
$sample = @(
    'aro-0.0.0-JSD1QtuBNwCASyBtNF3pqTl_W3oAJQGEVyFAtrBSE_Pa',
    'translate_c-2.0.0-Q_BUWltOBwBYABF84EUL208pIFeUIwbBs29FmVBaafN5'
)
foreach ($s in $sample) {
    $c = "$root\zig-system\$s"
    $z = "Z:\root\Desktop\tty\vt-verify\zig-system\$s"
    if ((Test-Path $c) -and (Test-Path $z)) {
        $fname = (Get-ChildItem $c -File | Sort-Object Name | Select-Object -First 1).Name
        $a = (Get-FileHash (Join-Path $c $fname) -Algorithm SHA256).Hash
        $b = (Get-FileHash (Join-Path $z $fname) -Algorithm SHA256).Hash
        Write-Host "sample $s/$fname : C=$a Z=$b : $(if ($a -eq $b) {'IDENTICAL'} else {'DIFFER'})"
    } else {
        Write-Host "sample missing: $s"
    }
}
$cr = Get-ChildItem "$root\zig-system" -Recurse -File -Include '*.zig', '*.zon' | Select-Object -First 50 |
    Where-Object { (Get-Content $_.FullName -Raw) -match "`r" }
Write-Host "CR-containing files among first 50 zig/zon sources: $($cr.Count) (0 = copy introduced no CRLF conversion)"
Write-Host "W2B DONE"
