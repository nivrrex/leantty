param([string]$AppBin = '')
. 'C:\dev\tty\scripts\windows\e2e\wb2_lib.ps1' -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$a = Start-App2 'pwshapp' (Join-Path $CFG 'pwsh-win.toml')
Start-Sleep -Seconds 6
$alive = $a.proc -and (-not $a.proc.HasExited)
$d = if (Test-Path $a.dump) { Get-Content $a.dump -Raw -Encoding UTF8 } else { '' }
Write-Host "APP alive=$alive bytes=$($d.Length)"
Write-Host ("DUMP_HEAD: " + (($d -split "`n")[1..3] -join ' | '))
Write-Host "ERR:"; Get-Content $a.err -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -Last 4
Stop-App2 $a.proc
