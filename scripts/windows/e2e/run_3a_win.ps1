# Windows 3A: incremental-vs-full redraw screenshot comparison, present stats,
# and responsiveness under a heavy output burst.
param([string]$AppBin = '')
. (Join-Path $PSScriptRoot 'wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Drawing

function Start-Case2([string]$name, [string]$cfgName) {
    $a = Start-App2 $name (Join-Path $CFG $cfgName)
    if ($a.hwnd -ne [IntPtr]::Zero) { $null = Focus-App2 $a.hwnd }
    $a
}

# capture the app's own client area (pid-filtered window, never other windows)
function Shoot-Client([IntPtr]$hwnd, [string]$path) {
    $cr = New-Object WB2+RECT
    if (-not [WB2]::GetClientRect($hwnd, [ref]$cr)) { return $false }
    $pt = New-Object WB2+POINT
    $pt.x = 0; $pt.y = 0
    [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
    $w = $cr.right - $cr.left; $h = $cr.bottom - $cr.top
    if ($w -le 0 -or $h -le 0) { return $false }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $gp = [System.Drawing.Graphics]::FromImage($bmp)
    # the window must be foreground for a clean grab; the case already focused it
    $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $gp.Dispose(); $bmp.Dispose()
    return $true
}

# ---------- a1: same input sequence, default incremental vs forced full redraw
function Run-Scene([string]$name, [bool]$forceFull) {
    if ($forceFull) { $env:APP_FORCE_FULL_REDRAW = '1' } else { Remove-Item Env:\APP_FORCE_FULL_REDRAW -ErrorAction SilentlyContinue }
    $a = Start-Case2 $name 'clip-win.toml'
    $Script:CurHwnd = $a.hwnd
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ ok = $false } }
    Send-Text2 'for /l %i in (1,1,40) do @echo L%i'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'L40' 40
    Start-Sleep -Milliseconds 1200
    # hide the caret so a blink cannot differ between the two grabs
    Send-Text2 'echo done'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'done' 25
    Start-Sleep -Milliseconds 1500
    $png = Join-Path $OUT "$name.png"
    $ok = Shoot-Client $a.hwnd $png
    Stop-App2 $a.proc
    Remove-Item Env:\APP_FORCE_FULL_REDRAW -ErrorAction SilentlyContinue
    @{ ok = $ok; png = $png }
}

$inc = Run-Scene 'a1-incremental' $false
Start-Sleep -Seconds 1
$full = Run-Scene 'a1-forced-full' $true
Start-Sleep -Seconds 1

$diffPixels = -1
$diffHead = ''
if ($inc.ok -and $full.ok) {
    $b1 = [System.Drawing.Bitmap]::FromFile($inc.png)
    $b2 = [System.Drawing.Bitmap]::FromFile($full.png)
    $w = [math]::Min($b1.Width, $b2.Width); $h = [math]::Min($b1.Height, $b2.Height)
    $diffPixels = 0
    $coords = @()
    for ($y = 0; $y -lt $h; $y++) {
        for ($x = 0; $x -lt $w; $x++) {
            if ($b1.GetPixel($x, $y).ToArgb() -ne $b2.GetPixel($x, $y).ToArgb()) {
                $diffPixels++
                if ($coords.Count -lt 5) { $coords += "($x,$y)" }
            }
        }
    }
    $diffHead = ($coords -join ' ')
    $b1.Dispose(); $b2.Dispose()
}
Report2 'a1-incremental-vs-full' ($inc.ok -and $full.ok -and $diffPixels -eq 0) "(size=$((Get-Item $inc.png).Length)/$((Get-Item $full.png).Length) diffPixels=$diffPixels first=$diffHead)"
Start-Sleep -Seconds 1

# ---------- a2: present timing, damage vs full buffer
function Run-Present([string]$name, [bool]$fullBuf) {
    if ($fullBuf) { $env:APP_PRESENT_FULL = '1' } else { Remove-Item Env:\APP_PRESENT_FULL -ErrorAction SilentlyContinue }
    $env:APP_PRESENT_STATS = '1'
    $a = Start-Case2 $name 'clip-win.toml'
    $Script:CurHwnd = $a.hwnd
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ ok = $false; lines = @() } }
    Send-Text2 'for /l %i in (1,1,40) do @echo P%i'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'P40' 40
    Start-Sleep -Seconds 3
    Stop-App2 $a.proc
    Remove-Item Env:\APP_PRESENT_FULL -ErrorAction SilentlyContinue
    Remove-Item Env:\APP_PRESENT_STATS -ErrorAction SilentlyContinue
    $lines = @()
    if (Test-Path $a.err) {
        $lines = (Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match 'present_us' })
    }
    @{ ok = $true; lines = $lines }
}

$pd = Run-Present 'a2-present-damage' $false
Start-Sleep -Seconds 1
$pf = Run-Present 'a2-present-full' $true
Report2 'a2-present-stats' ($pd.lines.Count -gt 0 -or $pf.lines.Count -gt 0) "(damageSamples=$($pd.lines.Count) fullSamples=$($pf.lines.Count))"
foreach ($l in $pd.lines | Select-Object -Last 3) { Write-Host "  damage: $l" }
foreach ($l in $pf.lines | Select-Object -Last 3) { Write-Host "  full  : $l" }
Start-Sleep -Seconds 1

# ---------- a3: 2000-line burst, then key echo latency (20 samples)
$a = Start-Case2 'a3-output-burst' 'clip-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2 'for /l %i in (1,1,2000) do @echo B%i'; Send-Vk2 0x0D
    $burstOk = Wait-Dump2 $a.dump 'B2000' 120
    Start-Sleep -Seconds 1
    $lat = @()
    # unique markers: no need to clear the input line, so the sample measures
    # only the echo round trip (8 backspaces would add ~0.6 s of its own)
    for ($i = 1; $i -le 20; $i++) {
        $marker = "MK$i"
        $t0 = Get-Date
        Send-Text2 $marker
        $seen = $false
        while (((Get-Date) - $t0).TotalMilliseconds -lt 3000) {
            $t = Get-Content $a.dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -and ($t -cmatch $marker)) { $seen = $true; break }
            Start-Sleep -Milliseconds 20
        }
        if ($seen) { $lat += ((Get-Date) - $t0).TotalMilliseconds } else { $lat += 3000 }
    }
    $sorted = $lat | Sort-Object
    $med = $sorted[[int][math]::Floor($sorted.Count / 2)]
    $p95 = $sorted[[int][math]::Floor($sorted.Count * 95 / 100)]
    Report2 'a3-burst-echo-latency' ($burstOk -and $med -lt 1000) "(burst2000=$burstOk echoMedianMs=$([int]$med) p95=$([int]$p95) max=$([int]($sorted[-1])))"
    Stop-App2 $a.proc
} else { Report2 'a3-burst-echo-latency' $false '(no window)' }

Write-Host "==== 3A-win render summary: PASS=$PASS FAIL=$FAIL ===="
if ($FAIL -gt 0) { exit 1 }
