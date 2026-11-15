param([string]$Kit = '', [int]$Lines = 30000)
$ErrorActionPreference = 'Stop'
. (Join-Path $Kit 'lib.ps1')
$app = Join-Path $Kit 'app.exe'
$cfg = Join-Path $Kit 'configs\cpu.toml'
$log = Join-Path $Kit 'diag-tp.log'
$big = Join-Path $Kit 'big-diag.txt'
$bytes = New-TestFile -Path $big -Lines $Lines
Write-Host ('DIAG file {0:N0} bytes' -f $bytes)
$h = Start-AppDirect -Exe $app -Config $cfg -LogPath $log -WorkDir $Kit
$hwnd = Wait-AppWindow -App $h.App -TimeoutMs 20000
Start-Sleep -Seconds 2
[void](Focus-Window $hwnd)
$short = Get-ShortPath $Kit
Send-Line ('cd /d ' + $short)
Start-Sleep -Milliseconds 800
$done = Join-Path $Kit 'tp-diag.done'
if (Test-Path $done) { Remove-Item $done -Force }
$t0 = Get-Date
Send-Line ('type big-diag.txt & echo DONE>tp-diag.done')
$ok = Wait-File $done 180000
$el = ((Get-Date) - $t0).TotalSeconds
[void](Focus-Window $hwnd)
Start-Sleep -Milliseconds 400
$shot = Join-Path $Kit 'diag-tp.png'
[void](Save-Shot $hwnd $shot)
Write-Host ('DIAG done={0} elapsed={1:N1}s' -f $ok, $el)
Stop-OwnApp $h.App $h.Cmd
Write-Host 'DIAG-DONE'
