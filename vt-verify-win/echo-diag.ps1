param([string]$AppBin = '')
. 'C:\dev\tty\scripts\windows\e2e\wb2_lib.ps1' -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$a = Start-App2 'echo-diag' (Join-Path $CFG 'clip-win.toml')
$null = Focus-App2 $a.hwnd
function Measure-Echo([string]$tag) {
    $t0 = Get-Date
    Send-Text2 $tag
    $seen = $false
    while (((Get-Date) - $t0).TotalMilliseconds -lt 5000) {
        $t = Get-Content $a.dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($t -and ($t -cmatch $tag)) { $seen = $true; break }
        Start-Sleep -Milliseconds 20
    }
    $ms = [int]((Get-Date) - $t0).TotalMilliseconds
    for ($b = 0; $b -lt 6; $b++) { Send-Vk2 0x08 }
    $ms
}
Write-Host "idle1=$(Measure-Echo 'aa') idle2=$(Measure-Echo 'bb') idle3=$(Measure-Echo 'cc')"
Send-Text2 'for /l %i in (1,1,2000) do @echo B%i'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'B2000' 120
Write-Host "burstDone"
Write-Host "during1=$(Measure-Echo 'dd') during2=$(Measure-Echo 'ee')"
Start-Sleep -Seconds 8
Write-Host "after1=$(Measure-Echo 'ff') after2=$(Measure-Echo 'gg')"
Stop-App2 $a.proc
