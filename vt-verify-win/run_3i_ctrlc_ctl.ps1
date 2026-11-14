param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\ps-win.toml'
$marker = Join-Path $ROOT 'vt-verify-win\osc\ctl-marker.txt'
Remove-Item $marker -ErrorAction SilentlyContinue
$a = Start-App2 'ctl' $cfg
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'CTL no-window'; exit }
$null = Focus-App2 $a.hwnd
Start-Sleep -Milliseconds 2500
Send-Text2 ('echo ok > ' + $marker); Send-Vk2 0x0D
$t0 = Get-Date; $seen = $false
while (((Get-Date) - $t0).TotalSeconds -lt 15) { if (Test-Path $marker) { $seen = $true; break }; Start-Sleep -Milliseconds 250 }
Write-Host ("CTL marker={0} afterMs={1}" -f $seen, [int]((Get-Date) - $t0).TotalMilliseconds)
Stop-App2 $a.proc
