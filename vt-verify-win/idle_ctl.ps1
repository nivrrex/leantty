$ErrorActionPreference='Continue'
$root="E:/dev/tty/vt-verify-win/tonight-kit-v4"
$env:APP_VT="conpty"; $env:APP_PERF_FRAMES="1"
$log="E:/dev/tty/vt-verify-win/logs/idle-ctl.log"
Remove-Item $log -ErrorAction SilentlyContinue
$p=Start-Process -FilePath "E:/dev/tty/target/release/app.exe" -ArgumentList @("--config","$root/configs/cpu.toml","--shell","bash","-c","while true; do echo tick; sleep 0.2; done") -RedirectStandardError $log -RedirectStandardOutput "E:/dev/tty/vt-verify-win/logs/idle-ctl-stdout.log" -WindowStyle Hidden -PassThru
Start-Sleep 5; $c1=(Get-Process -Id $p.Id).CPU
Start-Sleep 8; $c2=(Get-Process -Id $p.Id).CPU
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
$stats=@(Get-Content $log | Where-Object {$_ -match '\[perf\].*loops='})
Write-Output ("CONTROL_CPU_DELTA=" + [math]::Round(($c2-$c1),2))
Write-Output ("CONTROL_STATS_LINES=" + $stats.Count)
Write-Output ("LAST=" + ($stats | Select-Object -Last 1))
