param(
  [string]$Label,
  [string]$Exe,
  [string]$Cfg,
  [string]$Out,
  [string]$Corpus = 'C:\dev\tty\vt-verify-win\glyph-run\corpus.txt',
  [switch]$CovOnly
)
$ErrorActionPreference = 'Continue'
$log = Join-Path (Split-Path $Out) ("{0}.err" -f $Label)
$env:PYTHONIOENCODING = 'utf-8'
$args = @('--config-file', $Cfg, '-e', 'C:/Windows/py.exe', '-3', '-u',
          'C:/dev/tty/vt-verify-win/glyph-run/show.py', $Corpus)
$env:LEANTTY_FONT_COV_DEBUG = '1'
$p = Start-Process -FilePath $Exe -ArgumentList $args -PassThru -RedirectStandardError $log -RedirectStandardOutput ($log + '.out')
Start-Sleep -Seconds 6
if (-not $CovOnly) {
  & "$PSScriptRoot\shot_by_pid.ps1" -ProcId $p.Id -Out $Out | Write-Output
  Start-Sleep -Seconds 1
}
& taskkill.exe /T /F /PID $p.Id 2>$null | Out-Null
Start-Sleep -Milliseconds 500
Write-Output ("[{0}] log={1}" -f $Label, $log)
