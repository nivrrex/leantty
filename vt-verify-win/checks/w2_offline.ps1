# W2: offline reproducible build.
# a) GHOSTTY_SOURCE_DIR + GHOSTTY_ZIG_SYSTEM_DIR + cargo --offline
# b) REAL network block: per-program firewall rules PLUS a full outbound block.
#    (Per-program rules alone do not cover zig's child process maker.exe, which
#     performs the actual download - recorded as a verified fact from attempt 1.)
#    A self-healing scheduled task removes all rules if this script dies.
# c) record whether zig-system hash dirs are affected by line endings / path length
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$env:ZIG = "$root\checks\zig-log.cmd"
$env:ZIG_LOG_FILE = 'C:\vt\logs\w2-zig-args.log'
$env:GHOSTTY_SOURCE_DIR = "$root\vendor\ghostty-src"
$env:GHOSTTY_ZIG_SYSTEM_DIR = "$root\zig-system"
Remove-Item $env:ZIG_LOG_FILE -ErrorAction SilentlyContinue
$origProfiles = @()

try {
    Write-Host "=== W2a env ==="
    Write-Host "GHOSTTY_SOURCE_DIR=$env:GHOSTTY_SOURCE_DIR"
    Write-Host "GHOSTTY_ZIG_SYSTEM_DIR=$env:GHOSTTY_ZIG_SYSTEM_DIR"
    Write-Host "zig-system subdir count: $((Get-ChildItem $env:GHOSTTY_ZIG_SYSTEM_DIR -Directory).Count)"

    Write-Host "=== W2b firewall block ==="
    $origProfiles = Get-NetFirewallProfile | Select-Object Name,Enabled
    $origProfiles | Format-Table | Out-String | Write-Host
    Write-Host "(original profiles recorded above; restored in finally)"
    Set-NetFirewallProfile -Profile Domain,Private,Public -Enabled True
    Write-Host "profiles enabled for the duration of the block (original state: all Disabled)"
    $progs = @(
        'E:\dev\rust\cargo\bin\cargo.exe',
        'E:\dev\rust\cargo\bin\rustc.exe',
        "$root\toolchain\zig\zig.exe",
        'C:\Program Files\Git\cmd\git.exe'
    )
    foreach ($p in $progs) {
        if (Test-Path $p) {
            New-NetFirewallRule -DisplayName "W2_BLOCK_$(Split-Path $p -Leaf)" -Direction Outbound -Program $p -Action Block | Out-Null
            $bound = (Get-NetFirewallRule -DisplayName "W2_BLOCK_$(Split-Path $p -Leaf)" | Get-NetFirewallApplicationFilter).Program
            Write-Host "per-program rule added: $bound"
        }
    }
    New-NetFirewallRule -DisplayName 'W2_BLOCK_ALL_OUTBOUND' -Direction Outbound -Action Block | Out-Null
    Write-Host "full outbound block rule added: W2_BLOCK_ALL_OUTBOUND"

    # self-heal: remove all W2_BLOCK_* rules in 10 minutes even if this script dies
    $st = (Get-Date).AddMinutes(10).ToString('HH:mm')
    & schtasks /create /tn W2_UNBLOCK /tr "C:\vt\checks\w2_unblock.bat" /sc once /st $st /f | Out-Null
    Write-Host "self-heal task W2_UNBLOCK scheduled at $st"

    Write-Host "=== block proof A: zig fetch of UNCACHED reachable URL (must fail) ==="
    & "$root\toolchain\zig\zig.exe" fetch "https://ziglang.org/download/index.json" 2>&1 | Select-Object -First 6 | Tee-Object -FilePath "$root\logs\w2-block-zig.log"
    Write-Host "zig fetch exit=$LASTEXITCODE (nonzero = blocked)"

    Write-Host "=== block proof B: cargo search --registry crates-io (must fail) ==="
    Set-Location "$root\vendor\libghostty-rs"
    & cargo search serde --registry crates-io 2>&1 | Select-Object -First 6 | Tee-Object -FilePath "$root\logs\w2-block-cargo.log"
    Write-Host "cargo search exit=$LASTEXITCODE (nonzero = blocked)"

    Write-Host "=== block proof C: curl during full block (must fail) ==="
    & curl.exe -sS -m 15 -o NUL -w "curl during block: %{http_code}`n" https://ziglang.org/download/index.json 2>&1 | Tee-Object -FilePath "$root\logs\w2-block-curl.log"
    Write-Host "curl exit=$LASTEXITCODE (nonzero = blocked)"

    Write-Host "=== W2 offline build (cargo clean + --offline) ==="
    & cargo clean *>&1 | Out-Null
    & cargo build --workspace --offline *>&1 | Tee-Object -FilePath "$root\logs\w2-offline-build.log"
    $script:rc = $LASTEXITCODE
} finally {
    Write-Host "=== restore: remove all W2 firewall rules + self-heal task + profile states ==="
    Get-NetFirewallRule -DisplayName 'W2_BLOCK_*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule -Confirm:$false
    & schtasks /delete /tn W2_UNBLOCK /f 2>&1 | Out-Null
    if ($origProfiles) {
        foreach ($pr in $origProfiles) {
            Set-NetFirewallProfile -Name $pr.Name -Enabled ([bool]$pr.Enabled)
        }
    }
    $left = (Get-NetFirewallRule -DisplayName 'W2_BLOCK_*' -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "rules remaining (expect 0): $left"
    Get-NetFirewallProfile | Select-Object Name,Enabled | Format-Table | Out-String | Write-Host
}

if ($rc -ne 0) { Write-Host "W2 OFFLINE BUILD FAIL exit=$rc"; exit 1 }

Write-Host "=== restore proof: curl works again ==="
& curl.exe -sS -m 15 -o NUL -w "curl after restore: %{http_code}`n" https://ziglang.org/download/index.json 2>&1 | Tee-Object -FilePath "$root\logs\w2-restore-curl.log"

Write-Host "=== W2 zig invocation (wrapper log: --system present?) ==="
Get-Content $env:ZIG_LOG_FILE -ErrorAction SilentlyContinue | Tee-Object -FilePath "$root\logs\w2-zig-invocations.txt"

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
Write-Host "W2 DONE"
