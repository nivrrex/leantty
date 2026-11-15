# t3: full WARP (DX12 software adapter) verification on a GPU-less machine.
# The config is generated inline in pure ASCII (no Chinese comments, no encoding
# round trip) so the TOML section headers cannot be damaged.
# ASCII only.
param(
    [string]$Dir = 'C:\dev\tty\vt-verify-win\3q-warp',
    [string]$App = 'C:\dev\tty\target\release\app.exe',
    [int]$SoakMinutes = 5,
    [int]$Starts = 10
)
$ErrorActionPreference = 'Stop'
. 'C:\dev\tty\scripts\windows\tonight\lib.ps1'

function Stop-StaleApps([string]$marker) {
    Get-Process app -ErrorAction SilentlyContinue | ForEach-Object {
        $ci = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $_.Id) -ErrorAction SilentlyContinue
        if ($ci -and ([string]$ci.CommandLine) -like ('*' + $marker + '*')) {
            Stop-Process -Id $_.Id -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Milliseconds 800
}
Stop-StaleApps '3q-warp'
if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dir 'logs') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dir 'shots') | Out-Null

$cfgBody = @'
[window]
dimensions = { columns = 120, lines = 30 }
padding = { x = 0, y = 0 }
dynamic_title = true
opacity = 1.0

[scrolling]
history = 10000
multiplier = 10

[terminal]
shell = { program = "cmd.exe", args = [] }

[font]
size = 13.0
normal = { family = "Consolas", style = "Regular" }
bold = { family = "Consolas", style = "Bold" }
italic = { family = "Consolas", style = "Italic" }
bold_italic = { family = "Consolas", style = "Bold Italic" }
fallback = [ { family = "Microsoft YaHei", style = "Regular" } ]

[renderer]
backend = "gpu"

[selection]
save_to_clipboard = true

[colors.primary]
background = "#fcfcfc"
foreground = "#000000"

[colors.cursor]
text = "#fcfcfc"
cursor = "#0000ff"

[colors.normal]
black   = "#000000"
red     = "#d32f2f"
green   = "#008800"
yellow  = "#af8700"
blue    = "#0033cc"
magenta = "#c2185b"
cyan    = "#0097a7"
white   = "#757575"

[colors.bright]
black   = "#424242"
red     = "#ff0000"
green   = "#00aa00"
yellow  = "#fbc02d"
blue    = "#0000ff"
magenta = "#ff00ff"
cyan    = "#00e5ff"
white   = "#212121"

[[mouse.bindings]]
mouse = "Middle"
action = "Copy"

[[mouse.bindings]]
mouse = "Right"
action = "Paste"
'@
$gpuCfg = Join-Path $Dir 'gpu.toml'
$cfgBody | Set-Content -Path $gpuCfg -Encoding ASCII
$cpuCfg = Join-Path $Dir 'cpu.toml'
($cfgBody -replace 'backend = "gpu"', 'backend = "cpu"') | Set-Content -Path $cpuCfg -Encoding ASCII

$sample = Join-Path $Dir 'sample.ps1'
@(
    '$out = New-Object System.IO.StreamWriter("sample-done.txt", $false, (New-Object System.Text.UTF8Encoding($false)))',
    '$out.WriteLine("ascii")',
    '$out.Close()',
    'Write-Host ""',
    'Write-Host "ascii : ABC abc 0123 ~!@#$%^&*()"',
    'Write-Host ("cjk   : " + [string][char]0x4E2D + [string][char]0x6587 + [string][char]0x6D4B + [string][char]0x8BD5)',
    'Write-Host ("wide  : " + [string][char]0xFF21 + [string][char]0xFF22 + [string][char]0xFF23)',
    'Write-Host "colors:" -NoNewline',
    'for ($i = 31; $i -le 37; $i++) { Write-Host ("  " + [char]27 + "[" + $i + "mXX" + [char]27 + "[0m") -NoNewline }',
    'Write-Host ""',
    'Write-Host ([char]27 + "[7mINVERSE-LINE-" + [char]27 + "[0m normal)")',
    'Write-Host "END-OF-SAMPLE"'
) -join "`r`n" | Set-Content -Path $sample -Encoding ASCII -NoNewline
('@echo off' + "`r`n" + 'powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0sample.ps1"') |
    Set-Content -Path (Join-Path $Dir 'sample.cmd') -Encoding ASCII

$warpEnv = @{ APP_GPU_ALLOW_SOFTWARE = '1'; WGPU_BACKEND = 'dx12'; WGPU_ADAPTER_NAME = 'Basic' }
$cpuEnv = @{ WGPU_BACKEND = '' }
$short = Get-ShortPath $Dir
$rows = @()
function Note($o) { $script:rows += $o }

function Wait-PerfKey([string]$Log, [string]$Key, [int]$TimeoutMs = 30000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        $v = Get-PerfValue $Log $Key
        if ($null -ne $v) { return $v }
        Start-Sleep -Milliseconds 100
    }
    return $null
}

function Set-AppFocus([IntPtr]$Hwnd) {
    for ($i = 1; $i -le 3; $i++) {
        [void](Focus-Window $Hwnd)
        $g = Get-Geometry $Hwnd
        Click-Button ([int]($g.x + $g.w / 2)) ([int]($g.y + $g.h / 2)) 0x0002 0x0004
        Start-Sleep -Milliseconds 300
        if ([TN.Win]::GetForegroundWindow() -eq $Hwnd) { return $true }
    }
    return $false
}

function Confirm-Typing([IntPtr]$Hwnd, [string]$Command, [string]$Marker, [int]$TimeoutMs = 12000) {
    if (Test-Path $Marker) { Remove-Item $Marker -Force }
    for ($i = 1; $i -le 2; $i++) {
        [void](Set-AppFocus $Hwnd)
        Send-Line $Command
        if (Wait-File $Marker $TimeoutMs) { return $true }
    }
    return $false
}

function Shoot-Content([string]$tag, [string]$cfg, [hashtable]$env) {
    $log = Join-Path $Dir ('logs\content-' + $tag + '.log')
    $h = Start-AppDirect -Exe $App -Config $cfg -LogPath $log -WorkDir $Dir -EnvVars $env
    if (-not $h.App) { throw ('no app for ' + $tag) }
    $hwnd = Wait-AppWindow -App $h.App -TimeoutMs 30000
    if ($hwnd -eq [IntPtr]::Zero) { throw ('no window for ' + $tag) }
    [void](Focus-Window $hwnd)
    Start-Sleep -Milliseconds 700
    Send-Line ('cd /d ' + $short)
    Start-Sleep -Milliseconds 600
    $mk = Join-Path $Dir 'sample-done.txt'
    if (-not (Confirm-Typing $hwnd 'sample' $mk 12000)) { Write-Host ('T3B {0}: injection did not take effect' -f $tag) }
    Start-Sleep -Seconds 3
    $shot = Join-Path $Dir ('shots\S_{0}.png' -f $tag)
    [void](Save-Shot $hwnd $shot)
    return [pscustomobject]@{ Handle = $h; Hwnd = $hwnd; Shot = $shot; Log = $log }
}

# ---------------- a) N starts with WARP
$ff = @()
$firstLog = $null
for ($i = 1; $i -le $Starts; $i++) {
    $log = Join-Path $Dir ('logs\start-{0}.log' -f $i)
    $h = Start-AppDirect -Exe $App -Config $gpuCfg -LogPath $log -WorkDir $Dir -EnvVars $warpEnv
    if (-not $h.App) { Write-Host ('WARP start {0}: launch failed' -f $i); continue }
    $v = Wait-PerfKey $log 'first_frame_ms' 30000
    if ($null -ne $v) { $ff += $v }
    if (-not $firstLog) { $firstLog = $log }
    Start-Sleep -Milliseconds 300
    Stop-OwnApp $h.App $h.Cmd
    Start-Sleep -Milliseconds 800
}
$all = @(Get-Content $firstLog -Encoding UTF8)
$warn = @($all | Where-Object { $_ -match 'APP_GPU_ALLOW_SOFTWARE=1' })
$inst = Get-PerfLine $firstLog 'gpu_instance_ms'
$adapt = Get-PerfLine $firstLog 'gpu_adapter_ms'
$surf = Get-PerfLine $firstLog 'gpu_surface_ms'
$init = Get-PerfValue $firstLog 'gpu_init_ms'
$med = if ($ff.Count) { ($ff | Sort-Object)[[int]($ff.Count / 2)] } else { -1 }
$min = if ($ff.Count) { ($ff | Measure-Object -Minimum).Minimum } else { -1 }
Write-Host ('T3A starts={0} first_frame median={1:N0} min={2:N0}' -f $ff.Count, $med, $min)
Write-Host ('T3A warning_lines={0} :: {1}' -f $warn.Count, ($warn -join ' | '))
Write-Host ('T3A ' + $inst)
Write-Host ('T3A ' + $adapt)
Write-Host ('T3A ' + $surf)
Write-Host ('T3A gpu_init_ms=' + $init)
Note ([pscustomobject]@{ first_frame_median = $med; first_frame_min = $min; gpu_init_ms = $init; instance = $inst; adapter = $adapt; surface = $surf; warnings = $warn.Count })

# ---------------- b) content on both backends + pixel compare
$w = Shoot-Content 'warp' $gpuCfg $warpEnv
$c = Shoot-Content 'cpu' $cpuCfg $cpuEnv
$cmp = Compare-Png $c.Shot $w.Shot
Write-Host ('T3B cpu_vs_warp max_diff={0} diff_ratio={1} gt32_ratio={2} blank_rows={3} blank_cols={4} size={5}x{6}' -f $cmp.max_diff, $cmp.diff_ratio, $cmp.gt32_ratio, $cmp.blank_rows, $cmp.blank_cols, $cmp.w, $cmp.h)
Note ([pscustomobject]@{ cpu_vs_warp = $cmp })

# ---------------- c) idle 20 s: frame reports must stop
$before = @(Get-Content $w.Log -Encoding UTF8).Count
Start-Sleep -Seconds 20
$after = @(Get-Content $w.Log -Encoding UTF8)
$tail = if ($after.Count -gt $before) { $after[$before..($after.Count - 1)] } else { @() }
$gapLines = @($tail | Where-Object { $_ -match 'loop_gap_ms=' })
Write-Host ('T3C idle_reports={0}' -f $gapLines.Count)
foreach ($l in $gapLines) { Write-Host ('   | ' + $l) }
Note ([pscustomobject]@{ idle_reports = $gapLines.Count; idle_lines = ($gapLines -join ' || ') })

# ---------------- d) resize / minimize / cover
$g = Get-Geometry $w.Hwnd
[void](Set-AppFocus $w.Hwnd)
Start-Sleep -Milliseconds 400
$pre = Join-Path $Dir 'shots\S_warp_pre.png'
[void](Save-Shot $w.Hwnd $pre)
for ($i = 0; $i -lt 20; $i++) {
    $ww = if ($i % 2 -eq 0) { $g.w - 160 } else { $g.w }
    $hh2 = if ($i % 2 -eq 0) { $g.h - 100 } else { $g.h }
    [void][TN.Win]::SetWindowPos($w.Hwnd, [IntPtr]::Zero, $g.x, $g.y, $ww, $hh2, 0x0004)
    Start-Sleep -Milliseconds 250
}
[void][TN.Win]::SetWindowPos($w.Hwnd, [IntPtr]::Zero, $g.x, $g.y, $g.w, $g.h, 0x0004)
Start-Sleep -Milliseconds 800
[void](Set-AppFocus $w.Hwnd)
$post = Join-Path $Dir 'shots\S_warp_resized.png'
[void](Save-Shot $w.Hwnd $post)
$cmpResize = Compare-Png $pre $post
[void][TN.Win]::ShowWindow($w.Hwnd, 6)
Start-Sleep -Milliseconds 900
[void][TN.Win]::ShowWindow($w.Hwnd, 9)
[void](Set-AppFocus $w.Hwnd)
Start-Sleep -Milliseconds 900
$minShot = Join-Path $Dir 'shots\S_warp_min.png'
[void](Save-Shot $w.Hwnd $minShot)
$cmpMin = Compare-Png $post $minShot
Add-Type -AssemblyName System.Windows.Forms
$cover = New-Object System.Windows.Forms.Form
$cover.FormBorderStyle = 'None'
$cover.StartPosition = 'Manual'
$cover.Size = New-Object System.Drawing.Size ($g.w + 40), ($g.h + 40)
$cover.Location = New-Object System.Drawing.Point ([math]::Max(0, $g.x - 20)), ([math]::Max(0, $g.y - 20))
$cover.TopMost = $true
$cover.Show()
[System.Windows.Forms.Application]::DoEvents()
Start-Sleep -Milliseconds 1200
$cover.Close(); $cover.Dispose()
[void](Set-AppFocus $w.Hwnd)
Start-Sleep -Milliseconds 900
$coverShot = Join-Path $Dir 'shots\S_warp_cover.png'
[void](Save-Shot $w.Hwnd $coverShot)
$cmpCover = Compare-Png $minShot $coverShot
Write-Host ('T3D resize max_diff={0} ratio={1} blank_rows={2} blank_cols={3} | minimize max_diff={4} | cover max_diff={5}' -f $cmpResize.max_diff, $cmpResize.diff_ratio, $cmpResize.blank_rows, $cmpResize.blank_cols, $cmpMin.max_diff, $cmpCover.max_diff)
Note ([pscustomobject]@{ resize = $cmpResize; minimize = $cmpMin; cover = $cmpCover })

# ---------------- e) throughput 300k lines
$big = Join-Path $Dir 'big.txt'
if (-not (Test-Path $big)) { [void](New-TestFile -Path $big -Lines 300000) }
$tp = Join-Path $Dir 'tp.done'
if (Test-Path $tp) { Remove-Item $tp -Force }
[void](Set-AppFocus $w.Hwnd)
$t0 = Get-Date
Send-Line 'type big.txt & echo DONE>tp.done'
$ok = Wait-File $tp 900000
$el = ((Get-Date) - $t0).TotalSeconds
Start-Sleep -Seconds 3
$frames = Get-PerfFrames $w.Log
Write-Host ('T3E throughput done={0} seconds={1:N1} frames={2}' -f $ok, $el, ($frames | ConvertTo-Json -Compress))
Note ([pscustomobject]@{ throughput_ok = $ok; throughput_seconds = [math]::Round($el, 1); frames = $frames })

# ---------------- g) soak
$samples = @()
$sw2 = [Diagnostics.Stopwatch]::StartNew()
while ($sw2.Elapsed.TotalMinutes -lt $SoakMinutes) {
    Start-Sleep -Seconds 30
    $st = Get-AppStats $w.Handle.App.Id
    if ($st) {
        $samples += [pscustomobject]@{ t_s = [math]::Round($sw2.Elapsed.TotalSeconds); ws_mb = $st.ws_mb; priv_mb = $st.priv_mb; handles = $st.handles; threads = $st.threads }
    }
    [void](Set-AppFocus $w.Hwnd)
    Send-Line ('for /l %i in (1,1,100) do @echo soak')
}
Write-Host ('T3G soak samples={0}' -f $samples.Count)
foreach ($s in $samples) { Write-Host ('   t={0,4}s ws={1,6}MB priv={2,6}MB h={3} th={4}' -f $s.t_s, $s.ws_mb, $s.priv_mb, $s.handles, $s.threads) }
$samples | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Dir 'soak-warp.json') -Encoding UTF8
Note ([pscustomobject]@{ soak = $samples })

# ---------------- h) close timing + panic check
$panic = Test-LogHasPanic $w.Log
$closeSw = [Diagnostics.Stopwatch]::StartNew()
try { [void]$w.Handle.App.CloseMainWindow() } catch { }
$exited = $false
while ($closeSw.ElapsedMilliseconds -lt 15000) {
    $w.Handle.App.Refresh()
    if ($w.Handle.App.HasExited) { $exited = $true; break }
    Start-Sleep -Milliseconds 100
}
$closeSw.Stop()
if (-not $exited) { Stop-OwnApp $w.Handle.App $w.Handle.Cmd }
Stop-OwnApp $c.Handle.App $c.Handle.Cmd
Write-Host ('T3H closed={0} ms={1} panic={2}' -f $exited, [math]::Round($closeSw.Elapsed.TotalMilliseconds), $panic)
Note ([pscustomobject]@{ closed = $exited; close_ms = [math]::Round($closeSw.Elapsed.TotalMilliseconds); panic = $panic })

$notable = @(Get-Content $w.Log -Encoding UTF8 | Where-Object { $_ -match 'error|panic|Error|warn|surface' })
Write-Host ('T3H notable_lines={0}' -f $notable.Count)
foreach ($e in ($notable | Select-Object -First 8)) { Write-Host ('   | ' + $e) }
$rows | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Dir 't3-warp.json') -Encoding UTF8
Write-Host 'T3-DONE'
