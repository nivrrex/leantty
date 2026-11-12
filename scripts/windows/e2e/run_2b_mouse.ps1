# Windows 2B e2e part 2: mouse wheel, scrollbar drag, SGR reporting,
# Ctrl+Insert / Shift+Insert bindings (windows.toml style config).
param([string]$AppBin = '')
. (Join-Path $PSScriptRoot 'wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'

function Start-Case([string]$name, [string]$cfgName) {
    $a = Start-App2 $name (Join-Path $CFG $cfgName)
    if ($a.hwnd -ne [IntPtr]::Zero) { $null = Focus-App2 $a.hwnd }
    $a
}

function Count-Lines([string]$dump, [string]$prefix) {
    $t = Get-Content $dump -Raw -Encoding UTF8
    ([regex]::Matches($t, $prefix + '\d+')).Count
}

function Max-LineNo([string]$dump) {
    $t = Get-Content $dump -Raw -Encoding UTF8
    $m = [regex]::Matches($t, 'LN(\d+)')
    $best = -1
    foreach ($x in $m) {
        $v = [int]$x.Groups[1].Value
        if ($v -gt $best) { $best = $v }
    }
    $best
}

# ---------- k1: Ctrl+Insert copies the current selection.
# The drag itself already commits (save_to_clipboard=true), so clear the
# clipboard AFTER the drag: only the key binding can refill it.
$a = Start-Case 'k1-ctrl-insert-copy' 'clip-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2 'echo alpha beta gamma'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'alpha beta gamma' 25
    Start-Sleep -Milliseconds 700
    $fr = Find-RowRetry $a.dump 'alpha beta gamma' 8
    $row = $fr.row; $g = $fr.g
    if ($row -ge 0) {
        $geom = Get-Geom $a.hwnd
        $cp = Cell-XY $geom $g.cols $g.rows ($row + 2) 3
        Click-Left $cp.x $cp.y
        Start-Sleep -Milliseconds 400
        $cl = Cell-XY $geom $g.cols $g.rows $row 0
        $cr = Cell-XY $geom $g.cols $g.rows $row 17
        Drag-Left $cl.x $cl.y $cr.x $cr.y
        Start-Sleep -Milliseconds 400
        [System.Windows.Forms.Clipboard]::Clear()
        Start-Sleep -Milliseconds 200
        Set-Mod 0x11 $true
        Send-Vk2 0x2D
        Set-Mod 0x11 $false
        Start-Sleep -Milliseconds 600
    }
    $clip = [System.Windows.Forms.Clipboard]::GetText()
    Report2 'k1-ctrl-insert-copy' ($row -ge 0 -and $clip -eq 'alpha beta gamma') "(row=$row clip='$clip')"
    Stop-App2 $a.proc
} else { Report2 'k1-ctrl-insert-copy' $false '(no window)' }
Start-Sleep -Seconds 1

# ---------- k2: Shift+Insert -> Paste
$a = Start-Case 'k2-shift-insert-paste' 'clip-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    [System.Windows.Forms.Clipboard]::SetText('echo SHIFTINS_OK')
    Start-Sleep -Milliseconds 300
    Set-Mod 0x10 $true   # Shift down
    Send-Vk2 0x2D                                      # Insert
    Set-Mod 0x10 $false
    # the paste goes through the event loop: wait for the text to land first,
    # otherwise Enter races ahead of it
    $pasted = Wait-Dump2 $a.dump 'SHIFTINS_OK' 20
    Start-Sleep -Milliseconds 300
    Send-Vk2 0x0D
    Start-Sleep -Milliseconds 800
    $grid = (Dump-Grid $a.dump).grid
    $ran = ($grid | Where-Object { $_.Trim() -eq 'SHIFTINS_OK' }).Count -gt 0
    Report2 'k2-shift-insert-paste' ($pasted -and $ran) "(pasted=$pasted executed=$ran)"
    Stop-App2 $a.proc
} else { Report2 'k2-shift-insert-paste' $false '(no window)' }
Start-Sleep -Seconds 1

# ---------- w1: wheel up pauses auto-follow; new output does not pull the view
$a = Start-Case 'w1-wheel-autofollow' 'clip-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2 'for /l %i in (1,1,300) do @echo LN%i'; Send-Vk2 0x0D
    $okOut = Wait-Dump2 $a.dump 'LN300' 60
    Start-Sleep -Milliseconds 800
    $maxBottom = Max-LineNo $a.dump
    $geom = Get-Geom $a.hwnd
    Move-To ([int]($geom.ox + $geom.cw / 2)) ([int]($geom.oy + $geom.ch / 2))
    for ($i = 0; $i -lt 5; $i++) {
        [WB2]::mouse_event(0x0800, 0, 0, 120, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
    }
    Start-Sleep -Milliseconds 600
    $maxScrolled = Max-LineNo $a.dump
    Send-Text2 'echo afterscroll'; Send-Vk2 0x0D
    Start-Sleep -Seconds 2
    $maxAfter = Max-LineNo $a.dump
    $upWorked = ($maxScrolled -ge 0) -and ($maxScrolled -lt $maxBottom)
    $paused = ($maxAfter -le $maxScrolled + 2)
    Report2 'w1-wheel-autofollow' ($okOut -and $upWorked -and $paused) "(bottom=$maxBottom scrolled=$maxScrolled afterOut=$maxAfter)"
    Stop-App2 $a.proc
} else { Report2 'w1-wheel-autofollow' $false '(no window)' }
Start-Sleep -Seconds 1

# ---------- w2: scrollbar drag (right edge 4 px strip)
$a = Start-Case 'w2-scrollbar-drag' 'clip-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2 'for /l %i in (1,1,300) do @echo LN%i'; Send-Vk2 0x0D
    $okOut = Wait-Dump2 $a.dump 'LN300' 60
    Start-Sleep -Milliseconds 800
    $bottom = Max-LineNo $a.dump
    $geom = Get-Geom $a.hwnd
    $sx = $geom.ox + $geom.cw - 2
    # drag from mid-track up to the top of the track
    $yMid = $geom.oy + [int]($geom.ch / 2)
    $yTop = $geom.oy + 4
    [WB2]::mouse_event(0x0001 -bor 0x8000, (
        [uint32]([math]::Round(65535.0 * $sx / $SW))), (
        [uint32]([math]::Round(65535.0 * $yMid / $SH))), 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [WB2]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    for ($i = 1; $i -le 6; $i++) {
        $y = $yMid + [int](($yTop - $yMid) * $i / 6)
        [WB2]::mouse_event(0x0001 -bor 0x8000, (
            [uint32]([math]::Round(65535.0 * $sx / $SW))), (
            [uint32]([math]::Round(65535.0 * $y / $SH))), 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
    }
    [WB2]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 700
    $afterDrag = Max-LineNo $a.dump
    Report2 'w2-scrollbar-drag' ($okOut -and ($afterDrag -ge 0) -and ($afterDrag -lt $bottom)) "(bottom=$bottom afterDrag=$afterDrag)"
    Stop-App2 $a.proc
} else { Report2 'w2-scrollbar-drag' $false '(no window)' }
Start-Sleep -Seconds 1

# ---------- w3: ScrollToTop / ScrollToBottom bindings (scroll-win.toml)
$a = Start-Case 'w3-scroll-bindings' 'scroll-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2 'for /l %i in (1,1,300) do @echo LN%i'; Send-Vk2 0x0D
    $okOut = Wait-Dump2 $a.dump 'LN300' 60
    Start-Sleep -Milliseconds 900
    $bottom = Max-LineNo $a.dump
    $geom = Get-Geom $a.hwnd
    $cx = [int]($geom.ox + $geom.cw / 2)
    $cy = [int]($geom.oy + $geom.ch / 2)
    Move-To $cx $cy
    for ($i = 0; $i -lt 5; $i++) {
        [WB2]::mouse_event(0x0800, 0, 0, 120, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
    }
    Start-Sleep -Milliseconds 500
    $scrolled = Max-LineNo $a.dump
    Set-Mod 0x11 $true
    Send-Vk2 0x24   # Ctrl+Home -> ScrollToTop
    Set-Mod 0x11 $false
    Start-Sleep -Milliseconds 700
    Move-To ($cx + 2) ($cy + 2)
    Start-Sleep -Milliseconds 500
    $top = Max-LineNo $a.dump
    Set-Mod 0x11 $true
    Send-Vk2 0x23   # Ctrl+End -> ScrollToBottom
    Set-Mod 0x11 $false
    Start-Sleep -Milliseconds 700
    Move-To ($cx + 4) ($cy + 4)
    Start-Sleep -Milliseconds 500
    $back = Max-LineNo $a.dump
    $pulled = ($scrolled -ge 0) -and ($scrolled -lt $bottom)
    $toTop = ($top -ge 0) -and ($top -le 45)
    $toBottom = ($back -ge $bottom - 2)
    Report2 'w3-scroll-bindings' ($okOut -and $pulled -and $toTop -and $toBottom) "(bottom=$bottom scrolled=$scrolled top=$top back=$back)"
    Stop-App2 $a.proc
} else { Report2 'w3-scroll-bindings' $false '(no window)' }
Start-Sleep -Seconds 1

# ---------- s1: SGR mouse reporting. ConPTY consumes the SGR sequences on the
# input path (proved separately with vt-verify-win/probe_ptyin: writing
# ESC[<0;10;5M into the master delivers 0 bytes to the child), so the child
# cannot observe reporting on Windows. Assert that measured behaviour for all
# three tracking modes instead of pretending the report arrived.
function Invoke-Sgr([string]$modeArg, [string]$name) {
    $a = Start-Case $name 'clip-win.toml'
    $Script:CurHwnd = $a.hwnd
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ bytes = -1; done = $false } }
    Send-Text2 ('%TEMP%\pp.exe ' + $modeArg); Send-Vk2 0x0D
    $ready = Wait-Dump2 $a.dump 'READY' 25
    Start-Sleep -Milliseconds 600
    $g0 = (Dump-Grid $a.dump)
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom $g0.cols $g0.rows 3 10
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 300
    Move-To ($c.x + 8) ($c.y + 8)
    Start-Sleep -Milliseconds 300
    Click-Left $c.x $c.y
    # a left click with tracking off would select+copy; check that nothing was
    # reported to the child and the app stayed alive
    $null = Wait-Dump2 $a.dump 'BYTES=' 25
    $txt = Get-Content $a.dump -Raw -Encoding UTF8
    $bytes = -1
    if ($txt -match 'BYTES=(\d+)') { $bytes = [int]$Matches[1] }
    $alive = -not $a.proc.HasExited
    Stop-App2 $a.proc
    @{ bytes = $bytes; alive = $alive; ready = $ready }
}
$s1a = Invoke-Sgr 'mouse' 's1a-sgr-1000'
Start-Sleep -Seconds 1
$s1b = Invoke-Sgr 'm1002' 's1b-sgr-1002'
Start-Sleep -Seconds 1
$s1c = Invoke-Sgr 'm1003' 's1c-sgr-1003'
$allStripped = ($s1a.bytes -le 0) -and ($s1b.bytes -le 0) -and ($s1c.bytes -le 0)
$allAlive = $s1a.alive -and $s1b.alive -and $s1c.alive
$allReady = $s1a.ready -and $s1b.ready -and $s1c.ready
Report2 's1-sgr-conpty-strips' ($allStripped -and $allAlive -and $allReady) "(1000:bytes=$($s1a.bytes) 1002:bytes=$($s1b.bytes) 1003:bytes=$($s1c.bytes) alive=$allAlive)"

Write-Host "==== 2B mouse/sgr summary: PASS=$PASS FAIL=$FAIL ===="
if ($FAIL -gt 0) { exit 1 }
