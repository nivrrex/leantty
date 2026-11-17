# Manual app startup repro: env hook, dump, stderr, window.
$ErrorActionPreference = 'Continue'
$env:LEANTTY_DUMP_SNAPSHOT = 'C:\dev\tty\vt-verify-win\logs\manual.dump'
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath 'C:\dev\tty\target\debug\leantty.exe' -PassThru -RedirectStandardError 'C:\dev\tty\vt-verify-win\logs\manual.err' -WorkingDirectory 'C:\dev\tty'
Write-Host "pid=$($p.Id)"
Start-Sleep -Seconds 10
Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Select-Object Id,MainWindowTitle,Responding | Format-Table -AutoSize
Write-Host "===DUMP==="
Test-Path 'C:\dev\tty\vt-verify-win\logs\manual.dump'
Write-Host "===ERR==="
Get-Content 'C:\dev\tty\vt-verify-win\logs\manual.err' -ErrorAction SilentlyContinue | Select-Object -First 5
Write-Host "===DUMPCONTENT==="
Get-Content 'C:\dev\tty\vt-verify-win\logs\manual.dump' -ErrorAction SilentlyContinue | Select-Object -First 3
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
