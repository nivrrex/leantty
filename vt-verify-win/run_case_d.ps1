# Case-d reproduction: does the first synthetic keystroke get lost?
# Types "abcd", Left Left, Backspace, Enter and expects "acd".
param([string]$AppBin = '', [int]$Runs = 3)
$ROOT = 'C:\dev	ty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$ok = 0; $bad = 0
for ($i = 1; $i -le $Runs; $i++) {
    $a = Start-App2 "cased-$i" (Join-Path $CFG 'clip-win.toml')
    if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "run$i no-window"; continue }
    $null = Focus-App2 $a.hwnd
    Send-Text2 'abcd'; Send-Vk2 0x25; Send-Vk2 0x25; Send-Vk2 0x08; Send-Vk2 0x0D
    $hit = Wait-Dump2 $a.dump 'acd' 20
    $lost = Wait-Dump2 $a.dump 'cd' 2
    $text = if (Test-Path $a.dump) { Get-Content $a.dump -Raw -Encoding UTF8 } else { '' }
    $line = (($text -split "`n") | Where-Object { $_.Trim() -eq 'acd' -or $_.Trim() -eq 'cd' } | Select-Object -First 1)
    if ($hit) { $ok++ } else { $bad++ }
    Write-Host ("run{0} acd={1} line='{2}'" -f $i, $hit, ($line -replace "`r", ''))
    Stop-App2 $a.proc
    Start-Sleep -Seconds 1
}
Write-Host "CASE-D-SUMMARY ok=$ok bad=$bad"
