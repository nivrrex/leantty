# Fine-grained memory curve from process launch (20 ms cadence).
# stderr goes to a file so perf timestamps from the SAME run can be aligned later.
# Caveat: PowerShell file redirection costs ~6.8 ms per write (measured in t2), so
# absolute times are inflated; all alignment uses the app's own since_start marks.
# ASCII only.
param([string]$Exe = '', [string]$Cfg = '', [string]$Tag = 'A', [int]$Ms = 6000, [int]$StepMs = 20)
$ErrorActionPreference = 'Stop'
$dir = 'C:/dev/tty/vt-verify-win/logs/3l-mem'
$errFile = Join-Path $dir ("fine-$Tag.err")
if (Test-Path $errFile) { Remove-Item $errFile -Force }
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $Exe -ArgumentList @('--config', $Cfg) -PassThru -RedirectStandardError $errFile
$rows = New-Object System.Collections.Generic.List[string]
$rows.Add('t_ms,ws_mb,priv_mb') | Out-Null
while ($sw.Elapsed.TotalMilliseconds -lt $Ms) {
    $p.Refresh()
    if ($p.HasExited) { $rows.Add(('{0:F0},exited,exited' -f $sw.Elapsed.TotalMilliseconds)) | Out-Null; break }
    $rows.Add(('{0:F0},{1:F1},{2:F1}' -f $sw.Elapsed.TotalMilliseconds, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB))) | Out-Null
    Start-Sleep -Milliseconds $StepMs
}
$out = Join-Path $dir ("fine-$Tag.csv")
$rows -join "`n" | Set-Content -Path $out -Encoding ASCII
Write-Host ("FINE {0} rows={1}" -f $Tag, $rows.Count)
if (-not $p.HasExited) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
