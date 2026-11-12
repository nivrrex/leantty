# Windows 2B e2e part 1: clipboard (arboard Windows backend) + selection.
# Real input path: keybd_event + mouse_event (SendInput verified non-delivering).
# Cases: n1 drag-copy CJK, n2 paste CJK, n3 paste newlines, n4 paste 1MB,
#        n5 bracketed paste (?2004, PS) + n5b negative (cmd), n6 PasteSelection
#        (PRIMARY absent on Windows), m2 double-click word, m3 triple-click line,
#        m4 shift-click extend.
param([string]$AppBin = '')
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
if (-not $AppBin) { $AppBin = Join-Path $ROOT 'target\debug\app.exe' }
if (-not (Test-Path $AppBin)) { Write-Host "FATAL: no app binary"; exit 2 }
$OUT = Join-Path $ROOT 'vt-verify-win\logs\phase2b-win'
Copy-Item (Join-Path $ROOT 'vt-verify-win\rs.ps1') (Join-Path $env:TEMP 'rs.ps1') -Force
Copy-Item (Join-Path $ROOT 'vt-verify-win\rl.ps1') (Join-Path $env:TEMP 'rl.ps1') -Force
Copy-Item (Join-Path $ROOT 'vt-verify-win\probe_paste\target\release\pp.exe') (Join-Path $env:TEMP 'pp.exe') -Force
Copy-Item (Join-Path $ROOT 'vt-verify-win\osc123.ps1') (Join-Path $env:TEMP 'osc123.ps1') -Force
Copy-Item (Join-Path $ROOT 'vt-verify-win\osc999.ps1') (Join-Path $env:TEMP 'osc999.ps1') -Force
Copy-Item (Join-Path $ROOT 'vt-verify-win\osc123hold.ps1') (Join-Path $env:TEMP 'osc123hold.ps1') -Force
Copy-Item (Join-Path $ROOT 'vt-verify-win\osc999hold.ps1') (Join-Path $env:TEMP 'osc999hold.ps1') -Force
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$PASS = 0; $FAIL = 0
Add-Type -AssemblyName System.Windows.Forms

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WB2 {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint uCode, uint uMapType);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern short VkKeyScanW(char ch);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left; public int top; public int right; public int bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x; public int y; }
}
'@

[void][WB2]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$SW = [System.Windows.Forms.SystemInformation]::VirtualScreen.Width
$SH = [System.Windows.Forms.SystemInformation]::VirtualScreen.Height

$Script:FoundHwnd = [IntPtr]::Zero
$Script:FoundPid = 0
$EnumCb = [WB2+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WB2]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:FoundPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][WB2]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq $Script:FoundClass -and [WB2]::IsWindowVisible($h)) { $Script:FoundHwnd = $h }
    }
    return $true
}

function Start-App2([string]$name, [string]$configPath) {
    $dump = Join-Path $OUT "$name.dump"
    $err = Join-Path $OUT "$name.err"
    Remove-Item $dump, $err -ErrorAction SilentlyContinue
    $env:APP_DUMP_SNAPSHOT = $dump
    $argList = @('--config', $configPath)
    $p = Start-Process -FilePath $AppBin -ArgumentList $argList -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    $Script:FoundPid = [uint64]$p.Id
    $Script:FoundClass = 'Window Class'
    $Script:FoundHwnd = [IntPtr]::Zero
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 40) {
        [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
        if ($Script:FoundHwnd -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 500
    }
    $realHwnd = $Script:FoundHwnd
    # hide the app's own console window (console-subsystem app allocates one;
    # it steals focus during the case)
    $Script:FoundClass = 'ConsoleWindowClass'
    $Script:FoundHwnd = [IntPtr]::Zero
    [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
    if ($Script:FoundHwnd -ne [IntPtr]::Zero) { [void][WB2]::ShowWindow($Script:FoundHwnd, 0) }
    $Script:FoundClass = 'Window Class'
    if ($realHwnd -ne [IntPtr]::Zero) { Set-Guard $realHwnd }
    @{ proc = $p; hwnd = $realHwnd; dump = $dump; err = $err }
}

function Focus-App2([IntPtr]$hwnd) {
    $okFg = $false
    for ($i = 1; $i -le 5 -and -not $okFg; $i++) {
        [void][WB2]::ShowWindow($hwnd, 9)
        $tmp = 0
        $tid = [WB2]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
        $myTid = [WB2]::GetCurrentThreadId()
        if ($tid -ne $myTid) { [void][WB2]::AttachThreadInput($myTid, $tid, $true) }
        [void][WB2]::SetFocus($hwnd)
        [void][WB2]::SetForegroundWindow($hwnd)
        if ($tid -ne $myTid) { [void][WB2]::AttachThreadInput($myTid, $tid, $false) }
        Start-Sleep -Milliseconds 300
        $okFg = [WB2]::GetForegroundWindow() -eq $hwnd
    }
    return $okFg
}


# --- 输入安全闸门 -------------------------------------------------------------
# Every synthetic key/click must go to the app under test. If the foreground
# window is not the app (focus stolen by another terminal, IDE, browser...),
# injecting would type into that window instead - a 10 MB paste or Ctrl+C
# there can kill an unrelated session. So: refuse to inject unless the app
# window is foreground, and report loudly.
$Script:GuardHwnd = [IntPtr]::Zero
$Script:GuardViolations = 0

function Test-Guard {
    if ($Script:GuardHwnd -eq [IntPtr]::Zero) { return $true }
    if ([WB2]::GetForegroundWindow() -eq $Script:GuardHwnd) { return $true }
    $Script:GuardViolations++
    Write-Host "GUARD: foreground is not the app window (fg=$([WB2]::GetForegroundWindow()) app=$($Script:GuardHwnd)); input refused"
    return $false
}

function Set-Guard([IntPtr]$hwnd) { $Script:GuardHwnd = $hwnd }
function Clear-Guard { $Script:GuardHwnd = [IntPtr]::Zero }

function Assert-AppFocus([IntPtr]$hwnd, [string]$where) {
    for ($i = 0; $i -lt 3; $i++) {
        if ([WB2]::GetForegroundWindow() -eq $hwnd) { return $true }
        $null = Focus-App2 $hwnd
        Start-Sleep -Milliseconds 200
    }
    Write-Host "ABORT: app window never became foreground at [$where] (fg=$([WB2]::GetForegroundWindow()))"
    return $false
}

function Send-Vk2([byte]$vk) {
    if (-not (Test-Guard)) { return }
    # scan code 0 makes winit lose the logical key (ToUnicode fails), so map it;
    # Insert/Delete/Home/End/PgUp/PgDn/arrows are extended keys - without
    # KEYEVENTF_EXTENDEDKEY the scan code aliases the numpad key instead
    $scan = [byte]([WB2]::MapVirtualKeyW([uint32]$vk, 0) -band 0xFF)
    $ext = 0
    if (@(0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22, 0x25, 0x26, 0x27, 0x28) -contains [int]$vk) { $ext = 0x0001 }
    [WB2]::keybd_event($vk, $scan, $ext, [UIntPtr]::Zero)
    [WB2]::keybd_event($vk, $scan, 0x0002 -bor $ext, [UIntPtr]::Zero)
}


# Modifier key injection must carry the scan code: with scan 0 the keyup is
# not seen by winit and the modifier stays latched, which silently breaks
# every later keyboard binding lookup.
function Set-Mod([byte]$vk, [bool]$down) {
    $scan = [byte]([WB2]::MapVirtualKeyW([uint32]$vk, 0) -band 0xFF)
    $flags = if ($down) { 0 } else { 0x0002 }
    [WB2]::keybd_event($vk, $scan, $flags, [UIntPtr]::Zero)
}

function Send-Text2([string]$text) {
    foreach ($ch in $text.ToCharArray()) {
        $vk = [WB2]::VkKeyScanW($ch)
        if ($vk -lt 0) { continue }
        $vkb = [byte]($vk -band 0xFF)
        $shift = ($vk -band 0x0100) -ne 0
        if ($shift) { Set-Mod 0x10 $true }
        Send-Vk2 $vkb
        if ($shift) { Set-Mod 0x10 $false }
        Start-Sleep -Milliseconds 40
    }
}

function Wait-Dump2([string]$path, [string]$pattern, [int]$timeoutSec) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        if (Test-Path $path) {
            $c = Get-Content $path -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            # case-sensitive: otherwise AAAA matches leftover aaaa from a paste
            if ($c -and ($c -cmatch $pattern)) { return $true }
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

function Stop-App2($p) {
    Clear-Guard
    if ($p -and -not $p.HasExited) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 300
    }
}

# mouse helpers (absolute coords via mouse_event MOUSEEVENTF_ABSOLUTE)
function Move-To([int]$x, [int]$y) {
    $nx = [uint32]([math]::Round(65535.0 * $x / $SW))
    $ny = [uint32]([math]::Round(65535.0 * $y / $SH))
    [WB2]::mouse_event(0x0001 -bor 0x8000, $nx, $ny, 0, [UIntPtr]::Zero)
}
function Click-Btn([int]$x, [int]$y, [uint32]$down, [uint32]$up) {
    if (-not (Test-Guard)) { return }
    # tight timing: press-to-press must stay well under the 300 ms
    # multi-click window used by start_selection (crates/app/src/lib.rs)
    Move-To $x $y
    Start-Sleep -Milliseconds 15
    [WB2]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 15
    [WB2]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 25
}
function Click-Left([int]$x, [int]$y) { Click-Btn $x $y 0x0002 0x0004 }
function Click-Middle([int]$x, [int]$y) { Click-Btn $x $y 0x0020 0x0040 }
function Click-Right([int]$x, [int]$y) { Click-Btn $x $y 0x0008 0x0010 }
function Drag-Left([int]$x0, [int]$y0, [int]$x1, [int]$y1) {
    if (-not (Test-Guard)) { return }
    Move-To $x0 $y0
    Start-Sleep -Milliseconds 60
    [WB2]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    $steps = 8
    for ($i = 1; $i -le $steps; $i++) {
        $x = $x0 + [int](($x1 - $x0) * $i / $steps)
        $y = $y0 + [int](($y1 - $y0) * $i / $steps)
        Move-To $x $y
        Start-Sleep -Milliseconds 40
    }
    [WB2]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 120
}

# geometry: dump row/col -> client pixel (client rect from GetClientRect+ClientToScreen)
function Get-Geom([IntPtr]$hwnd) {
    $cr = New-Object WB2+RECT
    [void][WB2]::GetClientRect($hwnd, [ref]$cr)
    $pt = New-Object WB2+POINT
    $pt.x = 0; $pt.y = 0
    [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
    @{ cw = $cr.right - $cr.left; ch = $cr.bottom - $cr.top; ox = $pt.x; oy = $pt.y }
}
function Cell-XY([hashtable]$g, [int]$cols, [int]$rows, [int]$row, [int]$col) {
    $cellW = [math]::Max(1, [int]($g.cw / $cols))
    $cellH = [math]::Max(1, [int]($g.ch / $rows))
    @{ x = $g.ox + [int](($col + 0.5) * $cellW); y = $g.oy + [int](($row + 0.5) * $cellH); cellW = $cellW; cellH = $cellH }
}
function Dump-Grid([string]$path) {
    $lines = Get-Content $path -Encoding UTF8 -ErrorAction SilentlyContinue
    $hdr = $lines[0]
    $rows = 24; $cols = 80
    if ($hdr -match '# cols=(\d+) rows=(\d+)') { $cols = [int]$Matches[1]; $rows = [int]$Matches[2] }
    $grid = @()
    for ($i = 1; $i -le $rows; $i++) { $grid += $lines[$i] }
    @{ cols = $cols; rows = $rows; grid = $grid }
}
function Find-RowContaining([string[]]$grid, [string]$needle) {
    for ($i = 0; $i -lt $grid.Count; $i++) { if ($grid[$i].Contains($needle)) { return $i } }
    return -1
}
function Find-Row([string[]]$grid, [string]$exact) {
    for ($i = 0; $i -lt $grid.Count; $i++) { if ($grid[$i].Trim() -eq $exact) { return $i } }
    return -1
}

function Report2([string]$name, [bool]$ok, [string]$detail) {
    if ($ok) { $script:PASS++; Write-Host "PASS $name $detail" }
    else { $script:FAIL++; Write-Host "FAIL $name $detail" }
}


# find a row in the dump, retrying while the app rewrites the snapshot
function Find-RowRetry([string]$dump, [string]$text, [int]$tries) {
    for ($i = 0; $i -lt $tries; $i++) {
        $g = Dump-Grid $dump
        $r = Find-Row $g.grid $text
        if ($r -ge 0) { return @{ row = $r; g = $g } }
        Start-Sleep -Milliseconds 400
    }
    $g = Dump-Grid $dump
    return @{ row = -1; g = $g }
}
