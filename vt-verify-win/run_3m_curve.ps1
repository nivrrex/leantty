# Fine memory curve for an app started by bash (direct file-handle redirect for
# the [perf] log: `app.exe 2>file`, never PowerShell's Start-Process redirect).
# The caller starts this script FIRST in the background, then starts the app and
# writes its Windows PID into -PidFile, so sampling starts within tens of ms of
# process creation.
# ASCII only.
param(
    [string]$PidFile = '',
    [string]$OutCsv = '',
    [int]$Ms = 6000,
    [int]$StepMs = 20,
    [switch]$Long
)
$ErrorActionPreference = 'Stop'
if (-not $PidFile -or -not $OutCsv) { Write-Error 'need -PidFile and -OutCsv'; exit 2 }
# tell the caller we are ready to sample before the app is started
(Get-Date).Ticks | Set-Content -Path ($OutCsv + '.ready') -Encoding ASCII
$wait = [Diagnostics.Stopwatch]::StartNew()
$appPid = 0
while ($wait.Elapsed.TotalMilliseconds -lt 8000 -and $appPid -eq 0) {
    if (Test-Path $PidFile) {
        $txt = (Get-Content $PidFile -Raw -ErrorAction SilentlyContinue)
        if ($txt) { $appPid = [int]$txt.Trim() }
    }
    if ($appPid -eq 0) { Start-Sleep -Milliseconds 5 }
}
if ($appPid -eq 0) { Write-Error 'pid file never appeared'; exit 3 }
$attach = $wait.Elapsed.TotalMilliseconds
$rows = New-Object System.Collections.Generic.List[string]
$rows.Add('t_ms,ws_mb,priv_mb') | Out-Null
$sw = [Diagnostics.Stopwatch]::StartNew()
# -Long: 50 ms cadence for the first 5 s, then one sample every 5 s up to 60 s
$deadlineMs = $Ms
while ($sw.Elapsed.TotalMilliseconds -lt $deadlineMs) {
    $p = Get-Process -Id $appPid -ErrorAction SilentlyContinue
    if (-not $p) { $rows.Add(('{0:F0},exited,exited' -f $sw.Elapsed.TotalMilliseconds)) | Out-Null; break }
    $rows.Add(('{0:F0},{1:F1},{2:F1}' -f $sw.Elapsed.TotalMilliseconds, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB))) | Out-Null
    if ($Long) {
        $t = $sw.Elapsed.TotalSeconds
        if ($t -ge 60.0) { break }
        if ($t -lt 5.0) { $next = $t + 0.02 } else { $next = ([Math]::Floor($t / 5.0) + 1) * 5.0 }
        $sleepMs = [int](($next - $sw.Elapsed.TotalSeconds) * 1000)
        if ($sleepMs -gt 0) { Start-Sleep -Milliseconds $sleepMs }
    } else {
        Start-Sleep -Milliseconds $StepMs
    }
}
$rows -join "`n" | Set-Content -Path $OutCsv -Encoding ASCII
Write-Host ("CURVE {0} app_pid={1} attach_at={2:F0}ms samples={3}" -f (Split-Path -Leaf $OutCsv), $appPid, $attach, $rows.Count)
