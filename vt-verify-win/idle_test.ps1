$ErrorActionPreference='Continue'
$root="E:/dev/tty/vt-verify-win/tonight-kit-v4"
$env:LEANTTY_VT="conpty"; $env:LEANTTY_PERF_FRAMES="1"; $env:TERM="xterm"
$env:KN_CFG="$root/configs"; $env:KN_HOME=$root
$exe="E:/dev/tty/target/release/leantty.exe"
$log="E:/dev/tty/vt-verify-win/logs/idle-stats.log"
New-Item -ItemType Directory -Force "E:/dev/tty/vt-verify-win/logs" | Out-Null
Remove-Item $log -ErrorAction SilentlyContinue
$psi=@{
  FilePath=$exe
  ArgumentList=@("--config","$root/configs/cpu.toml","--shell","bash","-l")
  RedirectStandardError=$log
  RedirectStandardOutput="E:/dev/tty/vt-verify-win/logs/idle-stdout.log"
  WindowStyle="Hidden"; PassThru=$true
}
$p=Start-Process @psi
Start-Sleep 5
$cpu1=(Get-Process -Id $p.Id).CPU
Start-Sleep 13
$cpu2=(Get-Process -Id $p.Id).CPU
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep 1
$lines=Get-Content $log -ErrorAction SilentlyContinue
$stats=@($lines | Where-Object {$_ -match '\[perf\].*loops='})
Write-Output "CPU_DELTA_SEC=[math]::Round(($cpu2-$cpu1),3)"
Write-Output "STATS_LINES_TOTAL=$($stats.Count)"
Write-Output "LAST_STATS=$($stats | Select-Object -Last 2)"
Write-Output "TOTAL_LOG_LINES=$($lines.Count)"
