param([string]$Kit = '')
$ErrorActionPreference = 'Stop'
if (-not $Kit) { Write-Host 'DIAG need -Kit'; exit 2 }
. (Join-Path $Kit 'lib.ps1')
$app = Join-Path $Kit 'app.exe'
$cfg = Join-Path $Kit 'configs\cpu.toml'
$log = Join-Path $Kit 'diag-type.log'
$h = Start-AppDirect -Exe $app -Config $cfg -LogPath $log -WorkDir $Kit
if (-not $h.App) { Write-Host 'DIAG no app'; exit 2 }
$hwnd = Wait-AppWindow -App $h.App -TimeoutMs 20000
Start-Sleep -Seconds 3
$fg = Focus-Window $hwnd
Write-Host ("DIAG hwnd=$hwnd focus=$fg fg=$([TN.Win]::GetForegroundWindow())")
$a = Join-Path $Kit 'diag-a.png'
$b = Join-Path $Kit 'diag-b.png'
[void](Save-Shot $hwnd $a)
Start-Sleep -Milliseconds 500
Send-Text 'sample'
Start-Sleep -Milliseconds 200
Send-Vk2 0x0D
Start-Sleep -Seconds 3
[void](Focus-Window $hwnd)
Start-Sleep -Milliseconds 400
[void](Save-Shot $hwnd $b)
$c = Compare-Png $a $b
Write-Host ("DIAG diff_pixels={0} ratio={1} marker={2}" -f $c.diff_pixels, $c.diff_ratio, (Test-Path (Join-Path $Kit 'm.txt')))
Stop-OwnApp $h.App $h.Cmd
Write-Host 'DIAG-DONE'
