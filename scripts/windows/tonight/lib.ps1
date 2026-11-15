# lib.ps1 - helpers for run-tonight.ps1 (Windows tonight kit, v1).
#
# Rules honoured by every function here:
#   * no absolute paths, no network, no registry writes, no system settings
#   * only the app.exe started by this kit is ever stopped (by PID)
#   * OpenConsole.exe / conhost.exe / wezterm / pwsh processes are never touched
# ASCII only: Windows PowerShell 5.1 decodes BOM-less files as ANSI.
Set-StrictMode -Version 2.0

# ---------------------------------------------------------------- DPI / display
Add-Type -Namespace TN -Name Win -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[StructLayout(LayoutKind.Sequential)] public struct MONITORINFO { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }
public delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdc, ref RECT r, IntPtr data);
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool SetFocus(IntPtr h);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
[DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
[DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
[DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc cb, IntPtr data);
[DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr hm, ref MONITORINFO mi);
[DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr hm, int type, out uint x, out uint y);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern short VkKeyScanW(char ch);
[DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint mapType);
'@

function Initialize-DpiAware {
    # -4 = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 ; ignore failure on old builds
    $ok = [TN.Win]::SetProcessDpiAwarenessContext([IntPtr](-4))
    return $ok
}

function Get-Displays {
    Initialize-DpiAware | Out-Null
    $out = @()
    $cb = [TN.Win+MonitorEnumProc]{
        param($hm, $hdc, [ref]$r, $data)
        $mi = New-Object TN.Win+MONITORINFO
        $mi.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($mi)
        $dpiX = 0; $dpiY = 0
        [void][TN.Win]::GetDpiForMonitor($hm, 0, [ref]$dpiX, [ref]$dpiY)
        $name = ''
        if ([TN.Win]::GetMonitorInfo($hm, [ref]$mi)) {
            $name = ('{0}x{1}@({2},{3})' -f ($mi.rcMonitor.R - $mi.rcMonitor.L), ($mi.rcMonitor.B - $mi.rcMonitor.T), $mi.rcMonitor.L, $mi.rcMonitor.T)
        }
        $script:__displays += , ([pscustomobject]@{
            monitor = $name
            dpiX = [int]$dpiX
            dpiY = [int]$dpiY
            scale = if ($dpiX -gt 0) { [math]::Round($dpiX / 96.0, 3) } else { 0 }
        })
        return $true
    }
    $script:__displays = @()
    [void][TN.Win]::EnumDisplayMonitors([IntPtr]::Zero, [IntPtr]::Zero, $cb, [IntPtr]::Zero)
    return $script:__displays
}

function Test-RdpSession {
    if ($env:SESSIONNAME -like 'RDP*') { return $true }
    try {
        $cs = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
        # Win32_ComputerSystem has no RDP flag; check the terminal session instead
    } catch { }
    try {
        $s = Get-CimInstance Win32_LogonSession -ErrorAction SilentlyContinue
        if ($s) { }
        $t = Get-CimInstance Win32_TerminalService -ErrorAction SilentlyContinue
        if ($t) { }
    } catch { }
    # qwinsta is read-only and present on all supported Windows builds
    try {
        $out = & query.exe session 2>$null
        foreach ($line in $out) {
            if ($line -match '>\s*(\S+)\s+(\d+)\s+Active') {
                if ($line -match 'rdp') { return $true }
            }
        }
    } catch { }
    return $false
}

# ---------------------------------------------------------------- app lifecycle
function Start-AppDirect {
    # Launches app.exe with a DIRECT file handle for stderr (cmd /c "... 2>log"),
    # because PowerShell's Start-Process -RedirectStandardError costs ~6.8 ms per
    # log line and inflates first_frame by 4x (PHASE3L/3M).
    # -EnvVars passes extra variables through cmd in the canonical quoted form
    # (set "K=V"), so no trailing space can leak into the value.
    param([string]$Exe, [string]$Config, [string]$LogPath, [string]$WorkDir, [hashtable]$EnvVars = @{})
    $wrapper = Join-Path $WorkDir ('launch-{0}.cmd' -f ([Guid]::NewGuid().ToString('N').Substring(0, 8)))
    $lines = @('@echo off')
    $lines += ('cd /d "{0}"' -f $WorkDir)
    $lines += 'set "APP_PERF_FRAMES=1"'
    foreach ($k in ($EnvVars.Keys | Sort-Object)) {
        $lines += ('set "{0}={1}"' -f $k, $EnvVars[$k])
    }
    $lines += ('"{0}" --config "{1}" 2>"{2}"' -f $Exe, $Config, $LogPath)
    # Default = ANSI (GBK on zh-CN): cmd.exe reads .cmd files in the ANSI code page,
    # so a kit path with non-ASCII characters survives only in this encoding
    $lines -join "`r`n" | Set-Content -Path $wrapper -Encoding Default
    $cmd = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', ('"{0}"' -f $wrapper)) -PassThru -WindowStyle Hidden
    # resolve the app.exe child of that cmd (retry: process creation is async)
    $app = $null
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 15000 -and -not $app) {
        $child = Get-CimInstance Win32_Process -Filter "Name='app.exe'" -ErrorAction SilentlyContinue |
            Where-Object { [int]$_.ParentProcessId -eq $cmd.Id } | Select-Object -First 1
        if ($child) {
            $app = Get-Process -Id ([int]$child.ProcessId) -ErrorAction SilentlyContinue
        } else {
            Start-Sleep -Milliseconds 25
        }
    }
    if ($app) {
        if (-not (Get-Variable -Name KitPids -Scope Script -ErrorAction SilentlyContinue)) { $script:KitPids = @() }
        $script:KitPids += $app.Id
    }
    return [pscustomobject]@{ Cmd = $cmd; App = $app; Wrapper = $wrapper; Log = $LogPath }
}

function Wait-AppWindow {
    # returns only when the window has a usable client area: an early handle still
    # reports 0x0 and saving a screenshot from it throws
    param($App, [int]$TimeoutMs = 20000, [int]$MinW = 200, [int]$MinH = 100)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        $App.Refresh()
        if ($App.HasExited) { return [IntPtr]::Zero }
        $h = $App.MainWindowHandle
        if ($h -ne [IntPtr]::Zero) {
            $r = New-Object TN.Win+RECT
            if ([TN.Win]::GetClientRect($h, [ref]$r)) {
                if (($r.R - $r.L) -ge $MinW -and ($r.B - $r.T) -ge $MinH) { return $h }
            }
        }
        Start-Sleep -Milliseconds 100
    }
    return [IntPtr]::Zero
}

function Stop-OwnApp {
    # Only ever stops the app object handed in (started by this kit, PID recorded).
    param($App, $Cmd)
    if ($App) {
        try {
            $App.Refresh()
            if (-not $App.HasExited) {
                [void]$App.CloseMainWindow()
                Start-Sleep -Milliseconds 800
                $App.Refresh()
                if (-not $App.HasExited) { Stop-Process -Id $App.Id -ErrorAction SilentlyContinue }
            }
        } catch { }
    }
    if ($Cmd) {
        try { if (-not $Cmd.HasExited) { Stop-Process -Id $Cmd.Id -ErrorAction SilentlyContinue } } catch { }
    }
}

function Stop-AllKitApps {
    # Stops ONLY apps this kit started (PIDs recorded by Start-AppDirect) and only
    # while their command line still points into the kit directory. Used before the
    # summary/zip step so a timed-out stage cannot leave a writer holding a log.
    param([string]$WorkDir)
    $stopped = @()
    if (-not (Get-Variable -Name KitPids -Scope Script -ErrorAction SilentlyContinue)) { return $stopped }
    foreach ($id in ($script:KitPids | Select-Object -Unique)) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if (-not $p) { continue }
        $ci = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $id) -ErrorAction SilentlyContinue
        $cmdline = if ($ci) { [string]$ci.CommandLine } else { '' }
        if ($WorkDir -and $cmdline -and ($cmdline -notlike ('*' + (Split-Path -Leaf $WorkDir) + '*'))) {
            continue
        }
        try { Stop-Process -Id $id -ErrorAction SilentlyContinue; $stopped += $id } catch { }
    }
    if ($stopped.Count -gt 0) { Start-Sleep -Milliseconds 800 }
    return $stopped
}

function Get-AppStats {
    param([int]$ProcId)
    $p = Get-Process -Id $ProcId -ErrorAction SilentlyContinue
    if (-not $p) { return $null }
    $p.Refresh()
    return [pscustomobject]@{
        ws_mb = [math]::Round($p.WorkingSet64 / 1MB, 1)
        priv_mb = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
        handles = $p.HandleCount
        threads = $p.Threads.Count
        cpu_s = [math]::Round($p.TotalProcessorTime.TotalSeconds, 2)
    }
}

# ---------------------------------------------------------------- focus / input
function Focus-Window {
    param([IntPtr]$Hwnd)
    for ($i = 1; $i -le 6; $i++) {
        [void][TN.Win]::ShowWindow($Hwnd, 9)   # SW_RESTORE
        $tid = [TN.Win]::GetWindowThreadProcessId($Hwnd, [IntPtr]::Zero)
        $my = [TN.Win]::GetCurrentThreadId()
        if ($tid -ne $my) { [void][TN.Win]::AttachThreadInput($my, $tid, $true) }
        [void][TN.Win]::SetFocus($Hwnd)
        [void][TN.Win]::SetForegroundWindow($Hwnd)
        if ($tid -ne $my) { [void][TN.Win]::AttachThreadInput($my, $tid, $false) }
        Start-Sleep -Milliseconds 200
        if ([TN.Win]::GetForegroundWindow() -eq $Hwnd) { return $true }
    }
    return $false
}

function Send-Vk2 {
    param([byte]$Vk)
    $scan = [byte]([TN.Win]::MapVirtualKeyW([uint32]$Vk, 0) -band 0xFF)
    $ext = 0
    if (@(0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22, 0x25, 0x26, 0x27, 0x28) -contains [int]$Vk) { $ext = 1 }
    [TN.Win]::keybd_event($Vk, $scan, $ext, [UIntPtr]::Zero)
    [TN.Win]::keybd_event($Vk, $scan, (0x0002 -bor $ext), [UIntPtr]::Zero)
}

function Set-Mod {
    param([byte]$Vk, [bool]$Down)
    $scan = [byte]([TN.Win]::MapVirtualKeyW([uint32]$Vk, 0) -band 0xFF)
    $flags = if ($Down) { 0 } else { 0x0002 }
    [TN.Win]::keybd_event($Vk, $scan, $flags, [UIntPtr]::Zero)
}

function Send-Text {
    param([string]$Text, [int]$CharDelayMs = 40)
    foreach ($ch in $Text.ToCharArray()) {
        $vk = [TN.Win]::VkKeyScanW($ch)
        if ($vk -lt 0) { continue }
        $vkb = [byte]($vk -band 0xFF)
        $shift = ($vk -band 0x0100) -ne 0
        if ($shift) { Set-Mod 0x10 $true }
        Send-Vk2 $vkb
        if ($shift) { Set-Mod 0x10 $false }
        Start-Sleep -Milliseconds $CharDelayMs
    }
}

function Send-Line {
    param([string]$Text, [int]$CharDelayMs = 40)
    Send-Text $Text $CharDelayMs
    Start-Sleep -Milliseconds 150
    Send-Vk2 0x0D
}

function Click-Button {
    param([int]$X, [int]$Y, [uint32]$Down, [uint32]$Up)
    [void][TN.Win]::SetCursorPos($X, $Y)
    Start-Sleep -Milliseconds 60
    [TN.Win]::mouse_event($Down, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [TN.Win]::mouse_event($Up, 0, 0, 0, [UIntPtr]::Zero)
}

function Get-Geometry {
    param([IntPtr]$Hwnd)
    $r = New-Object TN.Win+RECT
    [void][TN.Win]::GetWindowRect($Hwnd, [ref]$r)
    return [pscustomobject]@{ x = $r.L; y = $r.T; w = ($r.R - $r.L); h = ($r.B - $r.T) }
}

# ---------------------------------------------------------------- screenshots
function Invoke-ShotHelper {
    # runs shot.ps1 in a fresh process (per-monitor-v2 awareness is set at its very
    # first statement; in this process the awareness is already fixed)
    param([string]$Mode, [string]$Path = '', [IntPtr]$Hwnd = [IntPtr]::Zero)
    $helper = Join-Path $PSScriptRoot 'shot.ps1'
    # no embedded quotes: with the call operator every array element is one
    # argument, and a quoted string would arrive with the quotes as part of the path
    $args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $helper, '-Mode', $Mode)
    if ($Path) { $args += @('-Out', $Path) }
    if ($Hwnd -ne [IntPtr]::Zero) { $args += @('-Hwnd', $Hwnd.ToInt64().ToString()) }
    $out = & powershell.exe @args 2>&1
    $line = ($out | Where-Object { $_ -match '^\{' } | Select-Object -Last 1)
    if (-not $line) { throw ('shot.ps1 {0} produced no JSON: {1}' -f $Mode, ($out -join ' ')) }
    return ($line | ConvertFrom-Json)
}

function Save-Shot {
    param([IntPtr]$Hwnd, [string]$Path)
    $r = Invoke-ShotHelper -Mode 'shot' -Path $Path -Hwnd $Hwnd
    $props = $r.PSObject.Properties.Name
    if ($props -contains 'error') { throw ('screenshot failed: {0}' -f $r.error) }
    return $r.bytes
}

function Save-ShotLocal {
    param([IntPtr]$Hwnd, [string]$Path)
    Add-Type -AssemblyName System.Drawing
    $c = New-Object TN.Win+RECT
    if (-not [TN.Win]::GetClientRect($Hwnd, [ref]$c)) { throw 'GetClientRect failed' }
    $pt = New-Object TN.Win+POINT
    [void][TN.Win]::ClientToScreen($Hwnd, [ref]$pt)
    $w = $c.R - $c.L
    $h = $c.B - $c.T
    if ($w -le 0 -or $h -le 0) { throw ("client area invalid: {0}x{1}" -f $w, $h) }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    # CopyFromScreen on purpose: PrintWindow is black for DXGI flip-model windows;
    # the window must therefore be in the foreground (caller does that).
    $g.CopyFromScreen($pt.X, $pt.Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
    $g.Dispose()
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return (Get-Item $Path).Length
}

function ConvertTo-PixelArray {
    param([string]$Path)
    Add-Type -AssemblyName System.Drawing
    $bmp = [System.Drawing.Bitmap]::FromFile($Path)
    $rect = New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bytes = New-Object byte[] ($data.Stride * $bmp.Height)
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
    $bmp.UnlockBits($data)
    $w = $bmp.Width; $h = $bmp.Height; $stride = $data.Stride
    $bmp.Dispose()
    return [pscustomobject]@{ bytes = $bytes; w = $w; h = $h; stride = $stride }
}

function Get-BlackRatio {
    param([string]$Path)
    $img = ConvertTo-PixelArray $Path
    $n = 0; $black = 0
    for ($y = 0; $y -lt $img.h; $y += 4) {
        $row = $y * $img.stride
        for ($x = 0; $x -lt $img.w; $x += 4) {
            $i = $row + $x * 4
            $n++
            if ($img.bytes[$i] -lt 8 -and $img.bytes[$i + 1] -lt 8 -and $img.bytes[$i + 2] -lt 8) { $black++ }
        }
    }
    if ($n -eq 0) { return 1.0 }
    return [math]::Round($black / $n, 4)
}

function Compare-Png {
    # max channel difference, pixel ratios, and structural (all-blank row/col) diff
    param([string]$A, [string]$B)
    $ia = ConvertTo-PixelArray $A
    $ib = ConvertTo-PixelArray $B
    if ($ia.w -ne $ib.w -or $ia.h -ne $ib.h) {
        return [pscustomobject]@{ same_size = $false; max_diff = -1; diff_ratio = -1; gt32_ratio = -1; blank_rows = -1; blank_cols = -1; w = $ia.w; h = $ia.h }
    }
    $maxDiff = 0; $diff = 0; $gt32 = 0; $total = 0
    $rowBlank = New-Object bool[] $ia.h
    $colBlank = New-Object bool[] $ia.w
    for ($y = 0; $y -lt $ia.h; $y++) {
        $ro = $y * $ia.stride
        $rowAny = $false
        for ($x = 0; $x -lt $ia.w; $x++) {
            $i = $ro + $x * 4
            $d = 0
            for ($k = 0; $k -lt 3; $k++) {
                $dv = [math]::Abs([int]$ia.bytes[$i + $k] - [int]$ib.bytes[$i + $k])
                if ($dv -gt $d) { $d = $dv }
            }
            $total++
            if ($d -gt 0) { $diff++; $rowAny = $true; if (-not $colBlank[$x]) { } else { } }
            if ($d -gt 32) { $gt32++ }
            if ($d -gt $maxDiff) { $maxDiff = $d }
            if (-not $rowAny -and $d -gt 0) { $rowAny = $true }
        }
        $rowBlank[$y] = -not $rowAny
    }
    # a column is "structurally missing" when it differs on (almost) every row
    $cols = 0
    for ($x = 0; $x -lt $ia.w; $x++) {
        $all = $true
        for ($y = 0; $y -lt $ia.h; $y += 2) {
            $i = $y * $ia.stride + $x * 4
            $d = 0
            for ($k = 0; $k -lt 3; $k++) {
                $dv = [math]::Abs([int]$ia.bytes[$i + $k] - [int]$ib.bytes[$i + $k])
                if ($dv -gt $d) { $d = $dv }
            }
            if ($d -le 8) { $all = $false; break }
        }
        if ($all) { $cols++ }
    }
    $rows = 0
    for ($y = 0; $y -lt $ia.h; $y++) {
        $all = $true
        for ($x = 0; $x -lt $ia.w; $x += 2) {
            $i = $y * $ia.stride + $x * 4
            $d = 0
            for ($k = 0; $k -lt 3; $k++) {
                $dv = [math]::Abs([int]$ia.bytes[$i + $k] - [int]$ib.bytes[$i + $k])
                if ($dv -gt $d) { $d = $dv }
            }
            if ($d -le 8) { $all = $false; break }
        }
        if ($all) { $rows++ }
    }
    return [pscustomobject]@{
        same_size = $true
        max_diff = $maxDiff
        diff_ratio = if ($total) { [math]::Round($diff / $total, 5) } else { 0 }
        gt32_ratio = if ($total) { [math]::Round($gt32 / $total, 5) } else { 0 }
        blank_rows = $rows
        blank_cols = $cols
        diff_pixels = $diff
        total_pixels = $total
        w = $ia.w
        h = $ia.h
    }
}

# ---------------------------------------------------------------- perf parsing
function Get-PerfLine {
    param([string]$Log, [string]$Key)
    if (-not (Test-Path $Log)) { return $null }
    $hit = $null
    foreach ($l in (Get-Content $Log -Encoding UTF8 -ErrorAction SilentlyContinue)) {
        if ($l -match ("\[perf\] {0}=" -f [regex]::Escape($Key))) { $hit = $l }
    }
    return $hit
}

function Get-PerfValue {
    param([string]$Log, [string]$Key)
    $line = Get-PerfLine $Log $Key
    if (-not $line) { return $null }
    if ($line -match ("{0}=([0-9.]+)" -f [regex]::Escape($Key))) { return [double]$Matches[1] }
    return $null
}

function Get-PerfFrames {
    # the line is "[perf] frames n=50 ..." (no '=' after the key), so it needs its
    # own matcher instead of Get-PerfLine
    param([string]$Log)
    if (-not (Test-Path $Log)) { return $null }
    $line = $null
    foreach ($l in (Get-Content $Log -Encoding UTF8 -ErrorAction SilentlyContinue)) {
        if ($l -match '\[perf\] frames ') { $line = $l }
    }
    if (-not $line) { return $null }
    $o = @{}
    foreach ($k in 'n', 'coalesced', 'interval_ms', 'feed_bytes_per_s', 'chan_depth_max', 'loop_gap_max_ms', 'feed_ms_max') {
        if ($line -match ("{0}=([0-9.]+)" -f [regex]::Escape($k))) { $o[$k] = [double]$Matches[1] }
    }
    if ($line -match 'render_ms\(med=([0-9.]+)') { $o['render_med_ms'] = [double]$Matches[1] }
    if ($line -match 'present_ms\(med=([0-9.]+)') { $o['present_med_ms'] = [double]$Matches[1] }
    return $o
}

function Get-LogTail {
    param([string]$Log, [int]$Lines = 20)
    if (-not (Test-Path $Log)) { return @() }
    return @(Get-Content $Log -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -Last $Lines)
}

function Test-LogHasPanic {
    param([string]$Log)
    if (-not (Test-Path $Log)) { return $false }
    $c = Get-Content $Log -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $c) { return $false }
    return ($c -match 'panicked at' -or $c -match 'RUST_BACKTRACE' -or $c -match 'thread .* panicked')
}

# ---------------------------------------------------------------- misc
function New-TestFile {
    param([string]$Path, [int]$Lines = 300000, [string]$EndMark = 'END-MARK-3N')
    $sb = New-Object System.Text.StringBuilder
    for ($i = 1; $i -le $Lines; $i++) {
        [void]$sb.Append('line ').Append($i).Append(' : The quick brown fox jumps over the lazy dog 0123456789').Append("`r`n")
    }
    [void]$sb.Append($EndMark).Append("`r`n")
    [IO.File]::WriteAllText($Path, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
    return (Get-Item $Path).Length
}

function Wait-File {
    param([string]$Path, [int]$TimeoutMs = 60000)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        if (Test-Path $Path) { return $true }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Get-ShortPath {
    # 8.3 name (ASCII, no spaces) - typed commands cannot carry the kit path when
    # it contains spaces or Chinese characters
    param([string]$Path)
    try {
        $fso = New-Object -ComObject Scripting.FileSystemObject
        return $fso.GetFolder($Path).ShortPath.ToLower()
    } catch {
        return $Path
    }
}

function Get-Sha256 {
    param([string]$Path)
    (Get-FileHash -Path $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
