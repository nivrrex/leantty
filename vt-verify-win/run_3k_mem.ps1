param([string]$AppBin = '', [string]$Cfg = '', [string]$Tag = 'A', [int]$Runs = 3)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
for ($i = 1; $i -le $Runs; $i++) {
    $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError (Join-Path $OUT ("mem-$Tag-$i.err"))
    $t0 = Get-Date
    $res = @{}
    foreach ($t in 10, 30, 60) {
        while (((Get-Date) - $t0).TotalSeconds -lt $t) { Start-Sleep -Milliseconds 200 }
        $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
        if ($proc) {
            $res[$t] = ('{0}/{1}/{2}' -f [int]($proc.WorkingSet64/1MB), [int]($proc.PrivateMemorySize64/1MB), [int]($proc.PagedMemorySize64/1MB))
        } else { $res[$t] = 'exited' }
    }
    Write-Host ("T4 {0} run{1} 10s={2} 30s={3} 60s={4} (WS/Private/Paged MB)" -f $Tag, $i, $res[10], $res[30], $res[60])
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
}
