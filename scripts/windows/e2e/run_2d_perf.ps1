# Windows 2D: release startup latency for three configs, plus the moment the
# background (CJK fallback) font set lands. Both numbers come from the app's own
# [perf] lines, which are emitted on stderr regardless of build profile.
param([string]$AppBin = '')
. (Join-Path $PSScriptRoot 'wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'

function Measure-Config([string]$name, [string]$cfgPath, [int]$runs) {
    $firsts = @()
    $bgs = @()
    for ($i = 1; $i -le $runs; $i++) {
        $err = Join-Path $OUT "$name-$i.err"
        Remove-Item $err -ErrorAction SilentlyContinue
        Remove-Item Env:\LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue
        if ($cfgPath) {
            $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $cfgPath) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
        } else {
            $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
        }
        $ff = -1; $bg = -1
        $t0 = Get-Date
        while (((Get-Date) - $t0).TotalSeconds -lt 90) {
            if (Test-Path $err) {
                $t = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
                if ($t) {
                    if ($ff -lt 0 -and ($t -match 'first_frame_ms=(\d+)')) { $ff = [int]$Matches[1] }
                    if ($bg -lt 0 -and ($t -match 'bg_fonts_ms=(\d+)')) { $bg = [int]$Matches[1] }
                    if ($ff -ge 0 -and $bg -ge 0) { break }
                }
            }
            Start-Sleep -Milliseconds 150
        }
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Milliseconds 500
        $firsts += $ff
        $bgs += $bg
    }
    $fOk = $firsts | Where-Object { $_ -ge 0 } | Sort-Object
    $bOk = $bgs | Where-Object { $_ -ge 0 } | Sort-Object
    $fMed = -1; $fSpread = -1; $bMed = -1; $bSpread = -1
    if ($fOk.Count -gt 0) {
        $fMed = $fOk[[int][math]::Floor($fOk.Count / 2)]
        $fSpread = $fOk[$fOk.Count - 1] - $fOk[0]
    }
    if ($bOk.Count -gt 0) {
        $bMed = $bOk[[int][math]::Floor($bOk.Count / 2)]
        $bSpread = $bOk[$bOk.Count - 1] - $bOk[0]
    }
    @{ firsts = $firsts; bgs = $bgs; fMed = $fMed; fSpread = $fSpread; bMed = $bMed; bSpread = $bSpread }
}

$d = Measure-Config 'perf2d-default' $null 5
Report2 'd4-default' ($d.fMed -ge 0) "(first=($($d.firsts -join ',')) median=$($d.fMed)ms spread=$($d.fSpread)ms; bg=($($d.bgs -join ',')) median=$($d.bMed)ms)"

$w = Measure-Config 'perf2d-windowstoml' (Join-Path $CFG 'clip-win.toml') 5
Report2 'd4-cmd-yahei' ($w.fMed -ge 0) "(first=($($w.firsts -join ',')) median=$($w.fMed)ms spread=$($w.fSpread)ms; bg=($($w.bgs -join ',')) median=$($w.bMed)ms)"

$c = Measure-Config 'perf2d-consolas' (Join-Path $CFG 'consolas-win.toml') 5
Report2 'd4-consolas-yahei' ($c.fMed -ge 0) "(first=($($c.firsts -join ',')) median=$($c.fMed)ms spread=$($c.fSpread)ms; bg=($($c.bgs -join ',')) median=$($c.bMed)ms)"

Write-Host "==== 2D perf summary: PASS=$PASS FAIL=$FAIL ===="
if ($FAIL -gt 0) { exit 1 }
