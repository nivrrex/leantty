# Task 4 driver: run the full Windows e2e set under a given ConPTY condition.
# Conditions: system (no conpty.dll next to app.exe), wezterm, nodepity.
# The DLLs are staged next to the release binary only for the run and removed
# afterwards; nothing is committed.
param(
    [Parameter(Mandatory = $true)][string]$Condition,
    [string]$AppBin = '',
    [string]$WezDir = 'E:\Compiler\WezTerm',
    [string]$NodeDir = 'C:\Users\Admin\AppData\Roaming\npm\node_modules\openclaw\node_modules\@lydell\node-pty-win32-x64\prebuilds\win32-x64\conpty'
)
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
if (-not $AppBin) { $AppBin = Join-Path $ROOT 'target\debug\leantty.exe' }
$appDir = Split-Path -Parent $AppBin
$logs = Join-Path $ROOT 'vt-verify-win\logs\conpty-runs'
New-Item -ItemType Directory -Force -Path $logs | Out-Null

# reset: remove any staged conpty pair
foreach ($f in @('conpty.dll', 'OpenConsole.exe')) {
    $p = Join-Path $appDir $f
    if (Test-Path $p) { Remove-Item $p -Force }
}
switch ($Condition) {
    'system' { }
    'wezterm' {
        Copy-Item (Join-Path $WezDir 'conpty.dll') (Join-Path $appDir 'conpty.dll') -Force
        Copy-Item (Join-Path $WezDir 'OpenConsole.exe') (Join-Path $appDir 'OpenConsole.exe') -Force
    }
    'official' {
        $off = Join-Path $ROOT 'vt-verify-win\conpty-dl\official'
        Copy-Item (Join-Path $off 'conpty.dll') (Join-Path $appDir 'conpty.dll') -Force
        Copy-Item (Join-Path $off 'OpenConsole.exe') (Join-Path $appDir 'OpenConsole.exe') -Force
    }
    'nodepity' {
        Copy-Item (Join-Path $NodeDir 'conpty.dll') (Join-Path $appDir 'conpty.dll') -Force
        Copy-Item (Join-Path $NodeDir 'OpenConsole.exe') (Join-Path $appDir 'OpenConsole.exe') -Force
    }
    default { Write-Host "unknown condition: $Condition"; exit 2 }
}

function Run-Suite([string]$name, [string]$script, [string]$tag) {
    # no name-based process cleanup: each suite stops the PIDs it started itself
    Start-Sleep -Seconds 3
    $out = Join-Path $logs ("$tag-" + $name + ".log")
    & powershell.exe -STA -NoProfile -ExecutionPolicy Bypass -File $script 2>&1 | Out-File -Encoding utf8 $out
    $rc = $LASTEXITCODE
    $pass = (Select-String -Path $out -Pattern '^PASS ' -AllMatches | Measure-Object).Count
    $fail = (Select-String -Path $out -Pattern '^FAIL ' -AllMatches | Measure-Object).Count
    $failed = @(Select-String -Path $out -Pattern '^FAIL ' | ForEach-Object { $_.Line })
    Write-Host ("[{0}] {1}: rc={2} PASS={3} FAIL={4}" -f $tag, $name, $rc, $pass, $fail)
    foreach ($f in $failed) { Write-Host ("    " + $f) }
}

Run-Suite 'run_all' (Join-Path $ROOT 'scripts\windows\e2e\run_all.ps1') $Condition
Run-Suite '2b_clip' (Join-Path $ROOT 'scripts\windows\e2e\run_2b_clip.ps1') $Condition
Run-Suite '2b_mouse' (Join-Path $ROOT 'scripts\windows\e2e\run_2b_mouse.ps1') $Condition
Run-Suite '2c' (Join-Path $ROOT 'scripts\windows\e2e\run_2c_win.ps1') $Condition

# leave the app dir clean regardless of outcome
foreach ($f in @('conpty.dll', 'OpenConsole.exe')) {
    $p = Join-Path $appDir $f
    if (Test-Path $p) { Remove-Item $p -Force }
}
Write-Host ("CONDITION-DONE " + $Condition)
