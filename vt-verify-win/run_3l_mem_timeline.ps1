# Sample memory timeline of an already running process.
# The app is started from bash (stderr -> file so perf lines stay parseable),
# then this script polls WorkingSet64 / PrivateMemorySize64.
# ASCII only on purpose: Windows PowerShell 5.1 decodes non-BOM files as ANSI.
param([int]$ProcId = 0, [string]$OutCsv = '')
if ($ProcId -le 0) { Write-Error 'need -ProcId'; exit 2 }
if (-not $OutCsv) { Write-Error 'need -OutCsv'; exit 2 }
$sw = [Diagnostics.Stopwatch]::StartNew()
$rows = New-Object System.Collections.Generic.List[string]
$rows.Add('t_ms,ws_mb,priv_mb') | Out-Null
while ($sw.Elapsed.TotalSeconds -lt 62.0) {
    $p = Get-Process -Id $ProcId -ErrorAction SilentlyContinue
    if (-not $p) { $rows.Add(('{0:F0},dead,dead' -f ($sw.Elapsed.TotalSeconds * 1000))) | Out-Null; break }
    $t = $sw.Elapsed.TotalSeconds
    $p.Refresh()
    $rows.Add(('{0:F0},{1:F1},{2:F1}' -f ($t * 1000), ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB))) | Out-Null
    if ($t -lt 5.0) { $next = $t + 0.05 } else { $next = ([Math]::Floor($t / 5.0) + 1) * 5.0 }
    $sleepMs = [int](($next - $sw.Elapsed.TotalSeconds) * 1000)
    if ($sleepMs -gt 0) { Start-Sleep -Milliseconds $sleepMs }
}
$rows -join "`n" | Set-Content -Path $OutCsv -Encoding ASCII
Write-Host ("MEMCSV {0} rows={1}" -f $OutCsv, $rows.Count)
