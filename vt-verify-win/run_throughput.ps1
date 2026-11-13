param([string]$AppBin = '', [string]$Tag = 'system', [int]$Runs = 2)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$file = Join-Path $ROOT 'vt-verify-win\perfdata\big200k.txt'
$env:APP_PERF_FRAMES = '1'
for ($i = 1; $i -le $Runs; $i++) {
  $a = Start-App2 ("tp-$Tag-$i") (Join-Path $CFG 'clip-win.toml')
  if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "tp run$i no-window"; continue }
  $null = Focus-App2 $a.hwnd
  Start-Sleep -Milliseconds 800
  # warm up the shell so the measurement is the file transfer, not shell start
  Send-Text2 'echo warmup'; Send-Vk2 0x0D
  $null = Wait-Dump2 $a.dump 'warmup' 20
  Start-Sleep -Milliseconds 500
  # no '&' here: synthetic Shift chords latch SHIFT and corrupt the line
  $cmd = 'type "' + $file + '"'
  $t0 = Get-Date
  Send-Text2 $cmd
  Send-Vk2 0x0D
  $ok = Wait-Dump2 $a.dump 'ZZENDMARK' ([int]($env:TP_TIMEOUT_SEC))
  $ms = [int]((Get-Date) - $t0).TotalMilliseconds
  $frames = ''
  if (Test-Path $a.err) {
    $lines = Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match '\[perf\] frames' }
    if ($lines) { $frames = ($lines | Select-Object -Last 1) }
  }
  Write-Host ("TP {0} run{1} done={2} ms={3} | {4}" -f $Tag, $i, $ok, $ms, $frames)
  Stop-App2 $a.proc
  Start-Sleep -Seconds 1
}
