param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$a = Start-App2 't4d-min' (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "MIN no-window"; exit }
$null = Focus-App2 $a.hwnd
Send-Text2 'for /l %i in (1,1,20) do @echo D%i'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'D20' 40
Start-Sleep -Milliseconds 1000
Write-Host ("MIN beforeHeader: " + (Get-Content $a.dump -Encoding UTF8 -TotalCount 1))
[void][WB2]::ShowWindow($a.hwnd, 6)
Start-Sleep -Seconds 2
Write-Host ("MIN duringHeader: " + (Get-Content $a.dump -Encoding UTF8 -TotalCount 1))
[void][WB2]::ShowWindow($a.hwnd, 9)
Start-Sleep -Seconds 2
Write-Host ("MIN afterHeader: " + (Get-Content $a.dump -Encoding UTF8 -TotalCount 1))
Write-Host ("MIN afterDump: " + ((Get-Content $a.dump -Encoding UTF8 -TotalCount 4) -join ' | '))
Stop-App2 $a.proc
