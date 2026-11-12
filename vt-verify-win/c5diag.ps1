param([string]$AppBin = '')
. 'C:\dev\tty\scripts\windows\e2e\wb2_lib.ps1' -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$pout = Join-Path $OUT 'c5diag.txt'
Remove-Item $pout -ErrorAction SilentlyContinue
$env:PROBE_OUT = $pout
$a = Start-App2 'c5diag' (Join-Path $CFG 'clip-win.toml')
$null = Focus-App2 $a.hwnd
Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
$ready = Wait-Dump2 $a.dump 'READY' 25
Write-Host "ready=$ready out=$pout"
[System.Windows.Forms.Clipboard]::SetText('a' * (10 * 1024 * 1024))
Start-Sleep -Milliseconds 300
$g0 = Dump-Grid $a.dump
$geom = Get-Geom $a.hwnd
$c = Cell-XY $geom $g0.cols $g0.rows 1 40
Click-Left $c.x $c.y
Start-Sleep -Milliseconds 400
Click-Right $c.x $c.y
for ($i = 0; $i -lt 5; $i++) {
  Start-Sleep -Milliseconds 2500
  Send-Text2 'MARK'
  $cur = if (Test-Path $pout) { Get-Content $pout -Raw } else { '(none)' }
  Write-Host ("typed MARK #{0}: {1}" -f ($i+1), $cur)
}
for ($i = 0; $i -lt 120; $i++) {
  Start-Sleep -Seconds 1
  if (Test-Path $pout) {
    $cur = Get-Content $pout -Raw
    if ($i % 10 -eq 0) { Write-Host ("t={0}s {1}" -f $i, $cur) }
    if ($cur -match 'BYTES=10485780') { break }
  }
}
$final = if (Test-Path $pout) { Get-Content $pout -Raw } else { '(none)' }
Write-Host "FINAL: $final"
Stop-App2 $a.proc
