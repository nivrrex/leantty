param([string]$AppBin = '', [int]$Runs = 5)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
for ($i = 1; $i -le $Runs; $i++) {
    $err = Join-Path $OUT ("t6ext-$i.err")
    Remove-Item $err -ErrorAction SilentlyContinue
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    $tSpawn = [int]$sw.ElapsedMilliseconds
    $hwnd = [IntPtr]::Zero
    $tWin = -1
    while ($sw.ElapsedMilliseconds -lt 20000) {
        $Script:FoundPid = [uint64]$p.Id
        $Script:FoundClass = 'Window Class'
        $Script:FoundHwnd = [IntPtr]::Zero
        [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
        if ($Script:FoundHwnd -ne [IntPtr]::Zero) { $hwnd = $Script:FoundHwnd; $tWin = [int]$sw.ElapsedMilliseconds; break }
        Start-Sleep -Milliseconds 5
    }
    Start-Sleep -Milliseconds 800
    $ff = -1
    if (Test-Path $err) { $t = Get-Content $err -Raw -Encoding UTF8; if ($t -match 'first_frame_ms=(\d+)') { $ff = [int]$Matches[1] } }
    Write-Host ("T6EXT run{0} createMs={1} windowVisibleMs={2} firstFrameMs={3} gapMs={4}" -f $i, $tSpawn, $tWin, $ff, ($tWin - $ff))
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
}
