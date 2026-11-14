param([string]$AppBin = '', [int]$Runs = 5)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$cfg = Join-Path $ROOT 'examples\windows.toml'
for ($i = 1; $i -le $Runs; $i++) {
    $err = Join-Path $OUT ("t6j-$i.err")
    Remove-Item $err -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 40) {
        if (Test-Path $err) {
            $t = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -and ($t -match 'first_frame_ms=')) { break }
        }
        Start-Sleep -Milliseconds 100
    }
    Start-Sleep -Milliseconds 600
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 300
    $txt = if (Test-Path $err) { Get-Content $err -Raw -Encoding UTF8 } else { '' }
    $ff = -1; if ($txt -match 'first_frame_ms=([0-9.]+)') { $ff = [double]$Matches[1] }
    $grp = -1; if ($txt -match 'cpu_groups_ms=([0-9.]+)') { $grp = [double]$Matches[1] }
    $fdb = -1; if ($txt -match 'fontdb_ms=([0-9.]+)') { $fdb = [double]$Matches[1] }
    Write-Host ("T6J run{0} fontdb={1} groups={2} firstFrame={3}" -f $i, $fdb, $grp, $ff)
    foreach ($m in [regex]::Matches($txt, '\[perf\] font_seg_ms [^\n]*')) {
        $l = $m.Value
        $fam=''; $face=0; $rd=0; $pa=0
        if ($l -match "family='([^']+)'") { $fam = $Matches[1] }
        if ($l -match 'face_ms=([0-9.]+)') { $face = [double]$Matches[1] }
        if ($l -match 'read_ms=([0-9.]+)') { $rd = [double]$Matches[1] }
        if ($l -match 'parse_ms=([0-9.]+)') { $pa = [double]$Matches[1] }
        Write-Host ("    seg {0} face_ms={1} read_ms={2} parse_ms={3}" -f $fam, $face, $rd, $pa)
    }
    foreach ($m in [regex]::Matches($txt, '\[perf\] font_group_ms [^\n]*')) {
        $l = $m.Value
        $g=''; $fam=''; $fb=''; $tot=0
        if ($l -match 'group=(\d+)') { $g = $Matches[1] }
        if ($l -match "family='([^']+)'") { $fam = $Matches[1] }
        if ($l -match 'fallback=(\w+)') { $fb = $Matches[1] }
        if ($l -match 'total_ms=([0-9.]+)') { $tot = [double]$Matches[1] }
        Write-Host ("    grp{0} {1} fallback={2} total_ms={3}" -f $g, $fam, $fb, $tot)
    }
}
