# Fine memory curve (20 ms) for one release run.
# The app is launched through `cmd /c "... 2>file"` so the [perf] log goes to a
# DIRECT file handle (PowerShell Start-Process -RedirectStandardError costs about
# 6.8 ms per eprintln and must not be used for perf runs, see 3L 2.1 / 3M rule).
# The app PID is resolved as the child of the cmd process, so sampling starts
# within a few tens of milliseconds of process creation.
# ASCII only.
param(
    [string]$Exe = 'C:\dev\tty\target\release\leantty.exe',
    [string]$Cfg = 'C:\dev\tty\examples\windows.toml',
    [string]$Tag = 'A',
    [int]$Ms = 6000,
    [int]$StepMs = 20
)
$ErrorActionPreference = 'Stop'
$dir = 'C:\dev\tty\vt-verify-win\logs\3m-mem'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$errFile = Join-Path $dir ("fine-$Tag.err")
if (Test-Path $errFile) { Remove-Item $errFile -Force }

$inner = '"' + $Exe + '" --config "' + $Cfg + '" 2>"' + $errFile + '"'
$cmd = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', $inner) -PassThru -WindowStyle Hidden
$appPid = 0
$sw = [Diagnostics.Stopwatch]::StartNew()
while ($sw.Elapsed.TotalMilliseconds -lt 3000 -and $appPid -eq 0) {
    $child = Get-CimInstance Win32_Process -Filter "Name='app.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ParentProcessId -eq $cmd.Id } | Select-Object -First 1
    if ($child) { $appPid = [int]$child.ProcessId }
    else { Start-Sleep -Milliseconds 10 }
}
if ($appPid -eq 0) { Write-Error 'app.exe child not found'; exit 3 }
$spawnMs = $sw.Elapsed.TotalMilliseconds

$rows = New-Object System.Collections.Generic.List[string]
$rows.Add('t_ms,ws_mb,priv_mb') | Out-Null
$sw2 = [Diagnostics.Stopwatch]::StartNew()
while ($sw2.Elapsed.TotalMilliseconds -lt $Ms) {
    $p = Get-Process -Id $appPid -ErrorAction SilentlyContinue
    if (-not $p) { $rows.Add(('{0:F0},exited,exited' -f $sw2.Elapsed.TotalMilliseconds)) | Out-Null; break }
    $rows.Add(('{0:F0},{1:F1},{2:F1}' -f $sw2.Elapsed.TotalMilliseconds, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB))) | Out-Null
    Start-Sleep -Milliseconds $StepMs
}
$out = Join-Path $dir ("fine-$Tag.csv")
$rows -join "`n" | Set-Content -Path $out -Encoding ASCII
Write-Host ("FINEMEM {0} app_pid={1} attach_at={2:F0}ms samples={3}" -f $Tag, $appPid, $spawnMs, $rows.Count)
$p = Get-Process -Id $appPid -ErrorAction SilentlyContinue
if ($p) { $p.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 2; if (-not $p.HasExited) { Stop-Process -Id $appPid -ErrorAction SilentlyContinue } }
if (-not $cmd.HasExited) { Stop-Process -Id $cmd.Id -ErrorAction SilentlyContinue }
