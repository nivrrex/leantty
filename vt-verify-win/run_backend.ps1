param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
foreach ($b in @('gpu','auto')) {
  $a = Start-App2 ("backend-$b") (Join-Path $CFG ("backend-$b.toml"))
  Start-Sleep -Seconds 4
  $alive = $a.proc -and (-not $a.proc.HasExited)
  $win = $a.hwnd -ne [IntPtr]::Zero
  $err = if (Test-Path $a.err) { Get-Content $a.err -Raw -Encoding UTF8 } else { '' }
  $warn = ($err -split "`n" | Where-Object { $_ -match 'backend=|renderer|gpu' }) -join ' || '
  $backend = ''
  if ($err -match '\[perf\] backend=([a-z]+)') { $backend = $Matches[1] }
  Write-Host ("BACKEND {0}: window={1} alive={2} backendLine='{3}' warn='{4}'" -f $b, $win, $alive, $backend, $warn)
  if ($a.proc -and -not $a.proc.HasExited) { Stop-App2 $a.proc }
  Start-Sleep -Seconds 1
}
