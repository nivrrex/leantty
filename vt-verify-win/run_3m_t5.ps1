# t5: deployed dist with ONLY conpty.dll (OpenConsole.exe deleted) vs the full dist.
# For each case: is an injected command actually executed (marker file), which child
# processes does the app own (name/pid/command line), and is there any error output.
# The full dist is the control that proves the injection works.
# No OpenConsole/conhost/pwsh process is ended; only the app started here is closed.
# ASCII only.
param(
    [string]$Base = 'C:\dev\tty\vt-verify-win\logs\3m-deploy',
    [string]$CfgName = 'config.example.toml',
    [int]$TypeWaitSec = 4
)
$ErrorActionPreference = 'Stop'
. 'C:\dev\tty\scripts\windows\e2e\wb2_lib.ps1' -AppBin 'C:\dev\tty\target\release\app.exe'

function Start-App([string]$exe, [string]$cfg, [string]$workDir) {
    $si = New-Object System.Diagnostics.ProcessStartInfo
    $si.FileName = $exe
    $si.Arguments = '--config "' + $cfg + '"'
    $si.WorkingDirectory = $workDir
    $si.UseShellExecute = $false
    $si.RedirectStandardError = $false
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $si
    [void]$p.Start()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 20000) {
        $p.Refresh()
        if ($p.HasExited) { return $null }
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) { return $p }
        Start-Sleep -Milliseconds 150
    }
    return $null
}

function Get-Children([int]$parentId) {
    $out = @()
    foreach ($c in (Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)) {
        if ([int]$c.ParentProcessId -eq $parentId) { $out += $c }
    }
    return $out
}

function Run-Case([string]$label, [string]$dist, [string]$markerName) {
    $marker = Join-Path $dist $markerName
    if (Test-Path $marker) { Remove-Item $marker -Force }
    $p = Start-App (Join-Path $dist 'app.exe') (Join-Path $dist $CfgName) $dist
    if (-not $p) { Write-Host ("T5 {0} START-FAILED" -f $label); return }
    $focus = Focus-App2 $p.MainWindowHandle
    $assert = Assert-AppFocus $p.MainWindowHandle 'pre-type'
    Start-Sleep -Seconds $TypeWaitSec
    Send-Text2 ('echo MARK > ' + $markerName)
    Start-Sleep -Milliseconds 300
    Send-Vk2 0x0D
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $ok = $false
    while ($sw.Elapsed.TotalSeconds -lt 15) {
        if (Test-Path $marker) { $ok = $true; break }
        Start-Sleep -Milliseconds 200
    }
    $took = [int]$sw.Elapsed.TotalMilliseconds
    $p.Refresh()
    Write-Host ("T5 {0} pid={1} focus={2} assert={3} alive={4} marker_in_15s={5} took_ms={6}" -f $label, $p.Id, $focus, $assert, (-not $p.HasExited), $ok, $took)
    foreach ($c in (Get-Children $p.Id)) {
        Write-Host ("T5 {0} child name={1} pid={2} cmd={3}" -f $label, $c.Name, $c.ProcessId, $c.CommandLine)
    }
    if ((Get-Children $p.Id).Count -eq 0) { Write-Host ("T5 {0} child none" -f $label) }
    if ($ok) {
        Write-Host ("T5 {0} marker_bytes={1}" -f $label, (Get-Item $marker).Length)
    }
    $p.CloseMainWindow() | Out-Null
    Start-Sleep -Seconds 2
    $p.Refresh()
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 1
}

New-Item -ItemType Directory -Force -Path $Base | Out-Null
$zip = 'C:\dev\tty\vt-verify-win\tty-win-x64.zip'
$full = Join-Path $Base 'dist full'
$dllOnly = Join-Path $Base 'dist dll only'
$before = @()
Get-Process OpenConsole -ErrorAction SilentlyContinue | ForEach-Object { $before += $_.Id }
Write-Host ("T5 openconsole_before=[{0}]" -f ($before -join ','))

Run-Case 'full' $full 'm1.txt'
Run-Case 'dllonly' $dllOnly 'm1.txt'

Start-Sleep -Seconds 2
$after = @()
Get-Process OpenConsole -ErrorAction SilentlyContinue | ForEach-Object { $after += $_.Id }
$new = @($after | Where-Object { $before -notcontains $_ })
Write-Host ("T5 openconsole_after=[{0}] new=[{1}]" -f ($after -join ','), ($new -join ','))
foreach ($id in $new) {
    $pr = Get-Process -Id $id -ErrorAction SilentlyContinue
    $ci = Get-CimInstance Win32_Process -Filter "ProcessId=$id" -ErrorAction SilentlyContinue
    if ($pr) { Write-Host ("T5 residual openconsole pid={0} start={1} cmd={2}" -f $id, $pr.StartTime, $(if ($ci) { $ci.CommandLine } else { 'n/a' })) }
}
Write-Host "T5-DONE"
