$ErrorActionPreference='Continue'
$root="E:/dev/tty/vt-verify-win/tonight-kit-v4"
$env:LEANTTY_VT="conpty"; $env:LEANTTY_PERF_FRAMES="1"; $env:LEANTTY_GPU_ALLOW_SOFTWARE="1"
foreach($cfg in @("gpu.toml","gpu-auto.toml")){
  $log="E:/dev/tty/vt-verify-win/logs/gpu-probe-$cfg.log"
  Remove-Item $log -ErrorAction SilentlyContinue
  $p=Start-Process -FilePath "E:/dev/tty/target/release/leantty.exe" -ArgumentList @("--config","$root/configs/$cfg","--shell","bash","-c","echo probe; sleep 4") -RedirectStandardError $log -RedirectStandardOutput "E:/dev/tty/vt-verify-win/logs/gpu-probe-out.log" -WindowStyle Hidden -PassThru
  Start-Sleep 7
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Write-Output "=== $cfg ==="
  Get-Content $log -ErrorAction SilentlyContinue | Select-String -Pattern "backend|adapter|gpu|WGPU|surface|present|error|warn" | Select-Object -First 6 | ForEach-Object { $_.Line }
}
