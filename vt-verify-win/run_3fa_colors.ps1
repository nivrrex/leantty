# 3F-a: does the [colors] section of a config actually reach the screen?
# Starts the app with a given config, prints default-fg and ESC[31m text, and
# reports the dominant colours plus the brightest red. Only the app PID started
# here is ever stopped.
param(
    [string]$AppBin = '',
    [string]$Cfg = '',
    [string]$Tag = 'probe',
    [string]$ExpectBg = '',
    [string]$ExpectFg = '',
    [switch]$Inverse,
    [switch]$RedInverse
)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
$err = Join-Path $OUT "3fa-$Tag.err"
$dump = Join-Path $OUT "3fa-$Tag.dump"
Remove-Item $err, $dump -ErrorAction SilentlyContinue
$env:APP_DUMP_SNAPSHOT = $dump
$p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
$Script:FoundPid = [uint64]$p.Id
$Script:FoundClass = 'Window Class'
$Script:FoundHwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40 -and $Script:FoundHwnd -eq [IntPtr]::Zero) {
    [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
    if ($Script:FoundHwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 300 }
}
$hwnd = $Script:FoundHwnd
Write-Host ("3FA {0} pid={1} hwnd={2}" -f $Tag, $p.Id, $hwnd)
if ($hwnd -ne [IntPtr]::Zero) {
    $null = Focus-App2 $hwnd
    Start-Sleep -Milliseconds 600
    if ($RedInverse) {
        Send-Text2 ('powershell -NoProfile -Command "Write-Host -NoNewline ([char]27 + ''[7m'' + [char]27 + ''[31m'' + ''          '' + [char]27 + ''[0m'')"')
        Send-Vk2 0x0D
        Start-Sleep -Seconds 3
    } elseif ($Inverse) {
        # 10 inverse spaces: the cell fill should be the foreground colour
        Send-Text2 ('powershell -NoProfile -Command "Write-Host -NoNewline ([char]27 + ''[7m'' + ''          '' + [char]27 + ''[0m'')"')
        Send-Vk2 0x0D
        Start-Sleep -Seconds 3
    } else {
        # default foreground text
        Send-Text2 'echo PLAINTEXT'; Send-Vk2 0x0D
        Start-Sleep -Seconds 2
    }
    # ANSI red text
    Send-Text2 ('powershell -NoProfile -Command "Write-Host ([char]27 + ''[31m'' + ''REDTEXT'' + [char]27 + ''[0m'')"')
    Send-Vk2 0x0D
    Start-Sleep -Seconds 3
    $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($hwnd, [ref]$cr)
    $pt = New-Object WB2+POINT; $pt.x = 0; $pt.y = 0; [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
    $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
    $gp = [System.Drawing.Graphics]::FromImage($bmp)
    $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
    $png = Join-Path $OUT "3fa-$Tag.png"
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    $gp.Dispose(); $bmp.Dispose()
    # dominant colours and the most saturated red
    $hist = @{}
    $bestRed = 0; $bestRedPx = $null
    $b = [System.Drawing.Bitmap]::FromFile($png)
    for ($y = 0; $y -lt $b.Height; $y++) {
        for ($x = 0; $x -lt $b.Width; $x++) {
            $c = $b.GetPixel($x, $y)
            $key = "$($c.R),$($c.G),$($c.B)"
            if ($hist.ContainsKey($key)) { $hist[$key]++ } else { $hist[$key] = 1 }
            if ($c.R -gt 100 -and $c.R -gt ($c.G + 40) -and $c.R -gt ($c.B + 40) -and $c.R -gt $bestRed) {
                $bestRed = $c.R; $bestRedPx = "$($c.R),$($c.G),$($c.B)"
            }
        }
    }
    $b.Dispose()
    $top = $hist.GetEnumerator() | Sort-Object -Property Value -Descending | Select-Object -First 5
    Write-Host ("3FA {0} topColors: {1}" -f $Tag, (($top | ForEach-Object { "$($_.Key)x$($_.Value)" }) -join ' '))
    Write-Host ("3FA {0} mostSaturatedRed: {1}" -f $Tag, $bestRedPx)
    Write-Host ("3FA {0} expect bg={1} fg={2}" -f $Tag, $ExpectBg, $ExpectFg)
}
# stderr: look for config warnings
$text = if (Test-Path $err) { Get-Content $err -Raw -Encoding UTF8 } else { '' }
$warn = ($text -split "`n" | Where-Object { $_ -match 'config|color|警告|unknown|ignored|回落' }) -join ' | '
Write-Host ("3FA {0} configLines: {1}" -f $Tag, $(if ($warn) { $warn } else { '(none)' }))
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
Write-Host ("3FA {0} stoppedPid={1}" -f $Tag, $p.Id)
