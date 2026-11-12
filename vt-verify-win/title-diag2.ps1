param([string]$AppBin = '')
. 'C:\dev\tty\scripts\windows\e2e\wb2_lib.ps1' -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$a = Start-App2 'title-diag' (Join-Path $CFG 'clip-win.toml')
$null = Focus-App2 $a.hwnd
function Title([IntPtr]$h) { $sb = New-Object System.Text.StringBuilder 256; [void][WB2]::GetWindowText($h, $sb, 256); $sb.ToString() }
Write-Host "before='$(Title $a.hwnd)'"
Send-Text2 'powershell -NoProfile -File %TEMP%\osc123hold.ps1'; Send-Vk2 0x0D
$saw = $false
for ($i = 0; $i -lt 40; $i++) { Start-Sleep -Milliseconds 250; $t = Title $a.hwnd; if ($i % 4 -eq 0) { Write-Host ("poll[{0}]='{1}'" -f $i, $t) }; if ($t -eq 'T123') { $saw = $true; break } }
Write-Host "afterOscOnly='$(Title $a.hwnd)' saw=$saw"
# force a visible change: press Enter at the prompt
Send-Vk2 0x0D
$saw2 = $false
for ($i = 0; $i -lt 20; $i++) { Start-Sleep -Milliseconds 250; if ((Title $a.hwnd) -eq 'T123') { $saw2 = $true; break } }
Write-Host "afterVisibleChange='$(Title $a.hwnd)' saw=$saw2"
Stop-App2 $a.proc
