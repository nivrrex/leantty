param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$keys = @('config_ms','window_ms','surface_ms','pty_spawn_ms','first_pty_byte_ms','first_frame_ms','bg_fonts_ms')
function Measure-Phases([string]$name, [string]$cfgPath, [int]$runs) {
  for ($i = 1; $i -le $runs; $i++) {
    $err = Join-Path $OUT ("p3-$name-$i.err")
    Remove-Item $err -ErrorAction SilentlyContinue
    if ($cfgPath) { $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $cfgPath) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err }
    else { $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err }
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 30) {
      if (Test-Path $err) {
        $t = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($t) { $have = 0; foreach ($k in $keys) { if ($t -match ('\[perf\] ' + $k + '=')) { $have++ } }; if ($have -ge $keys.Count) { break } }
      }
      Start-Sleep -Milliseconds 150
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 300
    $txt = if (Test-Path $err) { Get-Content $err -Raw -Encoding UTF8 } else { '' }
    $row = @()
    foreach ($k in $keys) { $v = -1; if ($txt -match ('\[perf\] ' + $k + '=([0-9.]+)')) { $v = [double]$Matches[1] }; $row += "$k=$v" }
    $fdb = -1; if ($txt -match '\[perf\] fontdb_ms=([0-9.]+)') { $fdb = [double]$Matches[1] }
    $fsum = 0.0; foreach ($m in [regex]::Matches($txt, '\[perf\] font_ms=\d+ .*? ms=([0-9.]+)')) { $fsum += [double]$m.Groups[1].Value }
    $row += ("fontdb=" + $fdb); $row += ("fontsum=" + [math]::Round($fsum,1))
    Write-Host ("$name run$i " + ($row -join ' '))
  }
}
Measure-Phases 'default' $null 5
Measure-Phases 'windowstoml' (Join-Path $ROOT 'examples\windows.toml') 5
