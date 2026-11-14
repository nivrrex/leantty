param([string]$AppBin = '', [string]$Cfg = '', [string]$Tag = 'A', [int]$Runs = 5)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
for ($i = 1; $i -le $Runs; $i++) {
    $err = Join-Path $OUT ("t6-$Tag-$i.err")
    Remove-Item $err -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 40) {
        if (Test-Path $err) {
            $t = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -and ($t -match 'first_frame_ms=')) { break }
        }
        Start-Sleep -Milliseconds 100
    }
    Start-Sleep -Milliseconds 500
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 300
    $txt = if (Test-Path $err) { Get-Content $err -Raw -Encoding UTF8 } else { '' }
    $fdb = -1; if ($txt -match 'fontdb_ms=([0-9.]+)') { $fdb = [double]$Matches[1] }
    $grp = -1; if ($txt -match 'cpu_groups_ms=([0-9.]+)') { $grp = [double]$Matches[1] }
    $ff = -1;  if ($txt -match 'first_frame_ms=(\d+)') { $ff = [double]$Matches[1] }
    $groups = @()
    foreach ($m in [regex]::Matches($txt, '\[perf\] font_ms=\d+ family=''([^'']+)'' style=''([^'']+)'' used_style=''([^'']+)'' cached=(\w+) ms=([0-9.]+)')) {
        $groups += ("{0}/{1}={2}" -f $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[5].Value)
    }
    Write-Host ("T6 {0} run{1} fontdb={2} groups_ms={3} firstFrame={4} | {5}" -f $Tag, $i, $fdb, $grp, $ff, ($groups -join ' '))
}
