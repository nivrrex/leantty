# Windows 3A: startup phase breakdown from the app's [perf] lines.
# Five cold-ish runs per config; prints every phase per run plus medians.
param([string]$AppBin = '')
. (Join-Path $PSScriptRoot 'wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'

function Measure-Phases([string]$name, [string]$cfgPath, [int]$runs) {
    $rows = @()
    for ($i = 1; $i -le $runs; $i++) {
        $err = Join-Path $OUT "$name-$i.err"
        Remove-Item $err -ErrorAction SilentlyContinue
        if ($cfgPath) {
            $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $cfgPath) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
        } else {
            $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
        }
        $t0 = Get-Date
        $want = @('fontdb_ms', 'first_frame_ms', 'bg_fonts_ms', 'window_ms', 'surface_ms')
        while (((Get-Date) - $t0).TotalSeconds -lt 30) {
            if (Test-Path $err) {
                $t = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
                if ($t) {
                    # wait until every phase we report has been printed, so the
                    # background-font timestamp is not cut off by an early kill
                    $have = 0
                    foreach ($w in $want) { if ($t -match ("\[perf\] " + $w + "=")) { $have++ } }
                    if ($have -ge $want.Count) { break }
                }
            }
            Start-Sleep -Milliseconds 150
        }
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Milliseconds 400
        $txt = if (Test-Path $err) { Get-Content $err -Raw -Encoding UTF8 } else { '' }
        $row = [ordered]@{ run = $i }
        foreach ($w in @('fontdb_ms', 'window_ms', 'surface_ms', 'first_frame_ms', 'bg_fonts_ms')) {
            $v = -1
            if ($txt -match ("\[perf\] " + $w + "=([0-9.]+)")) { $v = [double]$Matches[1] }
            $row[$w] = $v
        }
        # sum of every group font load line (font_ms may appear once per family)
        $sum = 0.0; $n = 0
        foreach ($m in [regex]::Matches($txt, '\[perf\] font_ms=\d+ .*? ms=([0-9.]+)')) {
            $sum += [double]$m.Groups[1].Value; $n++
        }
        $row['font_groups'] = $n
        $row['font_sum_ms'] = [math]::Round($sum, 1)
        $rows += [pscustomobject]$row
    }
    $rows
}

function Show-Table($rows, [string]$title) {
    Write-Host "--- $title ---"
    foreach ($r in $rows) {
        Write-Host ("run{0} fontdb={1} fontGroups={2} fontSum={3} window={4} surface={5} firstFrame={6} bgFonts={7}" -f `
            $r.run, $r.fontdb_ms, $r.font_groups, $r.font_sum_ms, $r.window_ms, $r.surface_ms, $r.first_frame_ms, $r.bg_fonts_ms)
    }
    foreach ($k in @('fontdb_ms', 'font_sum_ms', 'window_ms', 'surface_ms', 'first_frame_ms', 'bg_fonts_ms')) {
        $vals = $rows | ForEach-Object { [double]$_.$k } | Where-Object { $_ -ge 0 } | Sort-Object
        if ($vals.Count -gt 0) {
            $med = $vals[[int][math]::Floor($vals.Count / 2)]
            Write-Host ("median {0}={1}" -f $k, $med)
        }
    }
}

$d = Measure-Phases 'ph-default' $null 5
Show-Table $d 'default'
$w = Measure-Phases 'ph-windowstoml' (Join-Path $ROOT 'examples\windows.toml') 5
Show-Table $w 'examples/windows.toml (Consolas + YaHei fallback, shell=pwsh)'

$okD = ($d | Where-Object { $_.first_frame_ms -ge 0 }).Count -gt 0
$okW = ($w | Where-Object { $_.first_frame_ms -ge 0 }).Count -gt 0
Report2 'e3-startup-phases' ($okD -and $okW) "(default runs=$($d.Count) config runs=$($w.Count))"
