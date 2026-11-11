# Windows e2e for phase 2A re-verification: app debug binary + real keyboard path.
# NOTE: SendInput is verified non-delivering on this host (returns success, zero
# events reach the app; probe15 evidence in vt-verify-win/logs/e2e-probe/), so input
# is injected via keybd_event (same kernel input stream); unicode via clipboard+Ctrl+V.
# One command runs all cases; each prints PASS/FAIL; failures keep dump + screenshot.
# Keyboard injection needs the app window foreground; if SetForegroundWindow never
# succeeds the script stops and reports (per spec).
param([string]$AppBin = '')
$ErrorActionPreference = 'Continue'
$ROOT = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
if (-not $AppBin) { $AppBin = Join-Path $ROOT 'target\debug\app.exe' }
if (-not (Test-Path $AppBin)) { Write-Host "FATAL: app binary not found: $AppBin"; exit 2 }
$OUT = Join-Path $PSScriptRoot 'out'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$PASS = 0; $FAIL = 0

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class Win {
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern short VkKeyScanW(char ch);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT {
        public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT {
        public uint uMsg; public ushort wParamL; public ushort wParamH;
    }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION {
        [FieldOffset(0)] public KEYBDINPUT ki;
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public HARDWAREINPUT hi;
    }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT {
        public uint type; public INPUTUNION u;
    }
}
'@

$WM_CLOSE = 0x0010
$KEYEVENTF_KEYUP = 0x0002
$KEYEVENTF_UNICODE = 0x0004
$SWP_NOMOVE = 0x0002
$SWP_NOZORDER = 0x0004
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object Win+INPUT))
# remove the Windows foreground lock timeout so SetForegroundWindow works from this background script
[void][Win]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)

# extended-key VKs (arrows/home/end/pgup/pgdn/insert/delete) need KEYEVENTF_EXTENDEDKEY
$EXTENDED_VK = @{ 0x21=$true; 0x22=$true; 0x23=$true; 0x24=$true; 0x25=$true; 0x26=$true; 0x27=$true; 0x28=$true; 0x2D=$true; 0x2E=$true; 0x6F=$true }

function Send-Vk([uint16]$vk) {
    $flags = 0
    if ($EXTENDED_VK.ContainsKey($vk)) { $flags = 0x0001 }
    [Win]::keybd_event([byte]$vk, 0, $flags, [UIntPtr]::Zero)
    [Win]::keybd_event([byte]$vk, 0, $flags -bor 0x0002, [UIntPtr]::Zero)
}

function Paste-Text([string]$text) {
    # unicode text via clipboard + Ctrl+V (keybd_event cannot inject unicode)
    $b64 = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($text))
    $cmd = "[void][Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); [System.Windows.Forms.Clipboard]::SetText([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('$b64')))"
    $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($cmd))
    Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-EncodedCommand', $enc) -Wait -WindowStyle Hidden
    [Win]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [Win]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
    [Win]::keybd_event(0x56, 0, 0x0002, [UIntPtr]::Zero)
    [Win]::keybd_event(0x11, 0, 0x0002, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
}

function Send-Text([string]$text) {
    $pending = @()
    foreach ($ch in $text.ToCharArray()) {
        $code = [uint32][char]$ch
        if ($code -ge 0x80) { $pending += $ch; continue }
        $vk = [Win]::VkKeyScanW($ch)
        if ($vk -lt 0) { $pending += $ch; continue }
        $vkb = [byte]($vk -band 0xFF)
        $shift = ($vk -band 0x0100) -ne 0
        if ($shift) { [Win]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero) }
        Send-Vk $vkb
        if ($shift) { [Win]::keybd_event(0x10, 0, 0x0002, [UIntPtr]::Zero) }
    }
    if ($pending.Count -gt 0) { Paste-Text (-join $pending) }
}

function Send-CtrlC() {
    [Win]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [Win]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
    [Win]::keybd_event(0x43, 0, 0x0002, [UIntPtr]::Zero)
    [Win]::keybd_event(0x11, 0, 0x0002, [UIntPtr]::Zero)
}

# real-window discovery: the pid owns several windows; only class "Window Class"
# (the winit app window, title "tty") accepts keyboard input. Get-Process
# MainWindowHandle instead returns the 6x6 hidden "Winit Thread Event Target".
$Script:FoundHwnd = [IntPtr]::Zero
$Script:FoundPid = 0
$Script:FoundClass = ''
$EnumCb = [Win+EnumProc] {
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][Win]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:FoundPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][Win]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq $Script:FoundClass -and [Win]::IsWindowVisible($h)) { $Script:FoundHwnd = $h }
    }
    return $true
}

function Get-MainHwnd([int]$targetPid, [int]$timeoutSec = 40) {
    $Script:FoundPid = [uint64]$targetPid
    $Script:FoundHwnd = [IntPtr]::Zero
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        $Script:FoundClass = 'Window Class'
        [void][Win]::EnumWindows($EnumCb, [IntPtr]::Zero)
        if ($Script:FoundHwnd -ne [IntPtr]::Zero) { return $Script:FoundHwnd }
        Start-Sleep -Milliseconds 500
    }
    return [IntPtr]::Zero
}

function Hide-ConsoleWindow([int]$targetPid) {
    # console-subsystem app allocates its own visible console window; hide it so it
    # cannot steal focus during the case.
    $Script:FoundPid = [uint64]$targetPid
    $Script:FoundHwnd = [IntPtr]::Zero
    $Script:FoundClass = 'ConsoleWindowClass'
    [void][Win]::EnumWindows($EnumCb, [IntPtr]::Zero)
    if ($Script:FoundHwnd -ne [IntPtr]::Zero) { [void][Win]::ShowWindow($Script:FoundHwnd, 0) }
}

function Wait-Foreground([IntPtr]$hwnd) {
    if ($null -eq $hwnd -or [IntPtr]::Zero -eq $hwnd) { return $false }
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 10) {
        [void][Win]::ShowWindow($hwnd, 9)
        # proven recipe (probe13/15): attach to the app's UI thread so SetFocus/
        # SetForegroundWindow take effect, then verify fg == hwnd.
        $tmp = 0
        $appTid = [Win]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
        $myTid = [Win]::GetCurrentThreadId()
        if ($appTid -ne $myTid) { [void][Win]::AttachThreadInput($myTid, $appTid, $true) }
        [void][Win]::SetFocus($hwnd)
        [void][Win]::SetForegroundWindow($hwnd)
        if ($appTid -ne $myTid) { [void][Win]::AttachThreadInput($myTid, $appTid, $false) }
        $fg2 = [Win]::GetForegroundWindow()
        if ($null -ne $fg2 -and [IntPtr]::Zero -ne $fg2 -and $fg2 -eq $hwnd) { return $true }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

function Read-DumpHeader([string]$path) {
    if (-not (Test-Path $path)) { return $null }
    $first = Get-Content $path -TotalCount 1 -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($first -match '# cols=(\d+) rows=(\d+) wide=(\d+) spacer=(\d+) alternate=(\w+)') {
        return @{ cols = [int]$Matches[1]; rows = [int]$Matches[2]; wide = [int]$Matches[3]; spacer = [int]$Matches[4]; alternate = $Matches[5] }
    }
    return $null
}

function Wait-Dump([string]$path, [string]$pattern, [int]$timeoutSec) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        if (Test-Path $path) {
            $c = Get-Content $path -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($c -and ($c -match $pattern)) { return $true }
        }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

function Wait-HeaderNeq([string]$path, [int]$cols, [int]$rows, [int]$timeoutSec) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        $h = Read-DumpHeader $path
        if ($h -and ($h.cols -ne $cols -or $h.rows -ne $rows)) { return $h }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Wait-Exit([System.Diagnostics.Process]$p, [int]$timeoutSec) {
    return $p.WaitForExit($timeoutSec * 1000)
}

function Stop-Case($p) {
    if ($p -and -not $p.HasExited) {
        $h = Get-MainHwnd ([int]$p.Id) 20
        if ($h -ne [IntPtr]::Zero) { [void][Win]::PostMessage($h, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) }
        if (-not $p.WaitForExit(5000)) { $p.Kill() }
    }
}

function Start-Case([string]$name, [string]$shell, [string]$configPath) {
    $dump = Join-Path $OUT "$name.dump"
    $err = Join-Path $OUT "$name.err"
    Remove-Item $dump -ErrorAction SilentlyContinue
    Remove-Item $err -ErrorAction SilentlyContinue
    $env:APP_DUMP_SNAPSHOT = $dump
    if ($shell) { $env:SHELL = $shell } else { Remove-Item Env:SHELL -ErrorAction SilentlyContinue }
    $argList = @()
    if ($configPath) { $argList = @('--config', $configPath) }
    if ($argList.Count -gt 0) {
        $p = Start-Process -FilePath $AppBin -ArgumentList $argList -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    } else {
        $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    }
    if (-not $p) {
        Write-Host "FAIL $name (Start-Process returned null)"
        $script:FAIL++
        return @{ ok = $false; proc = $null; hwnd = [IntPtr]::Zero; dump = $dump; err = $err }
    }
    $hwnd = Get-MainHwnd ([int]$p.Id)
    if ($hwnd -ne [IntPtr]::Zero) { Hide-ConsoleWindow ([int]$p.Id) }
    if ($hwnd -eq [IntPtr]::Zero) {
        Write-Host "FAIL $name (no main window for pid=$($p.Id); err: $((Get-Content $err -Raw -ErrorAction SilentlyContinue)))"
        $script:FAIL++
        Stop-Case $p
        return @{ ok = $false; proc = $p; hwnd = $hwnd; dump = $dump; err = $err }
    }
    return @{ ok = $true; proc = $p; hwnd = $hwnd; dump = $dump; err = $err }
}

function Report([string]$name, [bool]$ok, [string]$detail) {
    if ($ok) { Write-Host "PASS $name $detail"; $script:PASS++ }
    else {
        Write-Host "FAIL $name $detail"
        $script:FAIL++
    }
}

function Screenshot([string]$name) {
    try {
        Add-Type -AssemblyName System.Windows.Forms, System.Drawing
        $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
        $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
        $bmp.Save((Join-Path $OUT "$name-fail.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
    } catch { Write-Host "  screenshot failed: $_" }
}

# ---- case 0: release binary must not contain the debug-only hook string
$releaseBin = Join-Path $ROOT 'target\release\app.exe'
if (Test-Path $releaseBin) {
    $bytes = [System.IO.File]::ReadAllBytes($releaseBin)
    $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
    if ($ascii -match 'APP_DUMP_SNAPSHOT') { Report 'release-hook-gated' $false '(APP_DUMP_SNAPSHOT present in release binary)' }
    else { Report 'release-hook-gated' $true '(release binary has no APP_DUMP_SNAPSHOT string)' }
} else {
    Report 'release-hook-gated' $false '(target\release\app.exe missing; run build first)'
}

# ---- foreground gate: keyboard injection needs a real foreground window
$probe = Start-Case 'fgprobe' 'cmd.exe' $null
if (-not $probe.ok) {
    Write-Host 'FATAL: cannot start app for foreground probe; stop per spec'
    exit 2
}
if (-not (Wait-Foreground $probe.hwnd)) {
    Write-Host "FATAL: SetForegroundWindow never succeeded for hwnd=$($probe.hwnd); stop per spec (no interactive desktop?)"
    Stop-Case $probe.proc
    exit 2
}
Write-Host "INFO foreground window ready (pid=$($probe.proc.Id) hwnd=$($probe.hwnd)); keyboard injection available"
Stop-Case $probe.proc
Start-Sleep -Seconds 1

# ---- a: cmd.exe echo hello
$c = Start-Case 'case-a' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'echo hello'; Send-Vk 0x0D
    $ok = Wait-Dump $c.dump 'hello' 20
    Report 'a-cmd-echo-hello' $ok "(dump has hello)"
    if (-not $ok) { Screenshot 'case-a'; Write-Host "  dump: $(if (Test-Path $c.dump) { Get-Content $c.dump -Raw -Encoding UTF8 } else { '<missing>' })" }
    Stop-Case $c.proc
}

# ---- b: powershell.exe echo hello
$c = Start-Case 'case-b' 'powershell.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'echo hello'; Send-Vk 0x0D
    $ok = Wait-Dump $c.dump 'hello' 30
    Report 'b-powershell-echo-hello' $ok "(dump has hello)"
    if (-not $ok) { Screenshot 'case-b'; Write-Host "  dump: $(if (Test-Path $c.dump) { Get-Content $c.dump -Raw -Encoding UTF8 } else { '<missing>' })" }
    Stop-Case $c.proc
}

# ---- c: unicode echo ni hao, wide cells. ASCII keystrokes only: keybd_event cannot
# inject unicode and SendInput is non-delivering on this host, so the child shell
# prints the CJK string (grid rendering/wide-cell assertion is unchanged).
$c = Start-Case 'case-c' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'powershell -NoProfile -Command "[char]0x4F60+[char]0x597D"'; Send-Vk 0x0D
    $ok = Wait-Dump $c.dump ([char]0x4F60 + [char]0x597D) 40
    $h = Read-DumpHeader $c.dump
    $wideOk = $h -and $h.wide -ge 2 -and $h.spacer -ge 2
    Report 'c-cjk-wide' ($ok -and $wideOk) "(text=$ok wide=$($h.wide) spacer=$($h.spacer))"
    if (-not ($ok -and $wideOk)) { Screenshot 'case-c'; Write-Host "  dump: $(if (Test-Path $c.dump) { Get-Content $c.dump -Raw -Encoding UTF8 } else { '<missing>' })" }
    Stop-Case $c.proc
}

# ---- d: Backspace + arrow editing: abcd, Left, Left, Backspace -> acd
$c = Start-Case 'case-d' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'abcd'; Send-Vk 0x25; Send-Vk 0x25; Send-Vk 0x08; Send-Vk 0x0D
    $ok = Wait-Dump $c.dump 'acd' 20
    Report 'd-backspace-arrows' $ok "(dump has acd)"
    if (-not $ok) { Screenshot 'case-d'; Write-Host "  dump: $(if (Test-Path $c.dump) { Get-Content $c.dump -Raw -Encoding UTF8 } else { '<missing>' })" }
    Stop-Case $c.proc
}

# ---- e: WM_CLOSE then exit within 5s, no 'wake' in stderr
$c = Start-Case 'case-e' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'echo bye'; Send-Vk 0x0D
    $ok = Wait-Dump $c.dump 'bye' 20
    [void][Win]::PostMessage($c.hwnd, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
    $exited = Wait-Exit $c.proc 5
    $errText = if (Test-Path $c.err) { Get-Content $c.err -Raw } else { '' }
    $noWake = $errText -notmatch 'wake'
    Report 'e-wmclose-exit' ($exited -and $noWake) "(exited5s=$exited no-wake=$noWake)"
    if (-not ($exited -and $noWake)) { Screenshot 'case-e'; Write-Host "  stderr: $errText" }
    if (-not $exited) { Stop-Case $c.proc }
}

# ---- f: type exit then Enter, process exits within 5s
$c = Start-Case 'case-f' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'exit'; Send-Vk 0x0D
    $exited = Wait-Exit $c.proc 5
    $errText = if (Test-Path $c.err) { Get-Content $c.err -Raw } else { '' }
    $noWake = $errText -notmatch 'wake'
    Report 'f-exit-command' ($exited -and $noWake) "(exited5s=$exited no-wake=$noWake)"
    if (-not ($exited -and $noWake)) { Screenshot 'case-f'; Write-Host "  stderr: $errText" }
    Stop-Case $c.proc
}

# ---- g: resize window, cols/rows change, mode con output matches
$c = Start-Case 'case-g' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    $h0 = Read-DumpHeader $c.dump
    if (-not $h0) { Start-Sleep -Seconds 3; $h0 = Read-DumpHeader $c.dump }
    [void][Win]::SetWindowPos($c.hwnd, [IntPtr]::Zero, 0, 0, 1600, 1000, ($SWP_NOMOVE -bor $SWP_NOZORDER))
    $h1 = Wait-HeaderNeq $c.dump $h0.cols $h0.rows 15
    $okResize = $h1 -ne $null
    if ($okResize) {
        Send-Text 'mode con'; Send-Vk 0x0D
        $okMode = Wait-Dump $c.dump ("Columns:\s+$($h1.cols)\b") 15
        $okMode2 = Wait-Dump $c.dump ("Lines:\s+$($h1.rows)\b") 15
        Report 'g-resize-mode-consistent' ($okMode -and $okMode2) "(cols $($h0.cols)->$($h1.cols) rows $($h0.rows)->$($h1.rows) mode match=$okMode/$okMode2)"
    } else {
        Report 'g-resize-mode-consistent' $false '(header did not change after SetWindowPos)'
    }
    if (-not $okResize -or -not ($okMode -and $okMode2)) { Screenshot 'case-g'; Write-Host "  dump tail: $(if (Test-Path $c.dump) { (Get-Content $c.dump -Tail 4 -Encoding UTF8 | Out-String) } else { '<missing>' })" }
    Stop-Case $c.proc
}

# ---- h: SHELL unset -> default cmd.exe
$c = Start-Case 'case-h' '' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'echo DEFOK'; Send-Vk 0x0D
    $ok = Wait-Dump $c.dump 'DEFOK' 20
    Report 'h-default-shell-cmd' $ok "(dump has DEFOK)"
    if (-not $ok) { Screenshot 'case-h'; Write-Host "  dump: $(if (Test-Path $c.dump) { Get-Content $c.dump -Raw -Encoding UTF8 } else { '<missing>' })" }
    Stop-Case $c.proc
}

# ---- i/j/k: Ctrl-C via real keyboard path (keybd_event Ctrl+C)
function Run-CtrlC-Case([string]$name, [string]$shell, [string]$cmd) {
    $c = Start-Case $name $shell $null
    if (-not $c.ok) { return }
    [void](Wait-Foreground $c.hwnd)
    Send-Text $cmd; Send-Vk 0x0D
    if ($cmd -match 'Start-Sleep') { $pre = Wait-Dump $c.dump 'PS ' 20 } else { $pre = Wait-Dump $c.dump 'TTL=' 20 }
    $ttlBefore = 0
    if (Test-Path $c.dump) { $ttlBefore = ([regex]::Matches((Get-Content $c.dump -Raw -Encoding UTF8), 'TTL=')).Count }
    Send-CtrlC
    Start-Sleep -Seconds 3
    $ttlMid = 0
    if (Test-Path $c.dump) { $ttlMid = ([regex]::Matches((Get-Content $c.dump -Raw -Encoding UTF8), 'TTL=')).Count }
    Send-Text 'echo CCCDONE'; Send-Vk 0x0D
    $marker = Wait-Dump $c.dump 'CCCDONE' 10
    $ttlAfter = 0
    if (Test-Path $c.dump) { $ttlAfter = ([regex]::Matches((Get-Content $c.dump -Raw -Encoding UTF8), 'TTL=')).Count }
    $ok = $marker -and ($ttlAfter -le $ttlMid)
    Report $name $ok "(pre=$pre ttlBefore=$ttlBefore ttlMid=$ttlMid ttlAfter=$ttlAfter marker=$marker)"
    if (-not $ok) { Screenshot $name; Write-Host "  dump tail: $(if (Test-Path $c.dump) { (Get-Content $c.dump -Tail 6 -Encoding UTF8 | Out-String) } else { '<missing>' })" }
    Stop-Case $c.proc
}
# ping -t / Start-Sleep do not read stdin: the \x03 byte reaches the pty (case l
# proves the full chain on blocked sort) but only stdin readers observe it; this
# matches phase-1 E-series (E1-E12) and is re-confirmed here on the real path.
Run-CtrlC-Case 'i-cmd-ping-ctrl-c' 'cmd.exe' 'ping -t 127.0.0.1'
Run-CtrlC-Case 'j-powershell-ping-ctrl-c' 'powershell.exe' 'ping.exe -t 127.0.0.1'
Run-CtrlC-Case 'k-powershell-sleep-ctrl-c' 'powershell.exe' 'Start-Sleep -Seconds 100'

# ---- l: Ctrl+C positive control - blocked sort is interrupted (real-keyboard
# chain: keybd_event -> winit -> route_key -> encoder \x03 -> ConPTY -> cmd)
$c = Start-Case 'case-l' 'cmd.exe' $null
if ($c.ok) {
    [void](Wait-Foreground $c.hwnd)
    Send-Text 'sort'; Send-Vk 0x0D
    $blocked = Wait-Dump $c.dump 'sort' 20
    Send-CtrlC
    Start-Sleep -Seconds 3
    $grid = Get-Content $c.dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    $prompts = ([regex]::Matches($grid, 'C:\\Users\\Admin>')).Count
    $ok = $blocked -and $prompts -ge 2
    Report 'l-sort-ctrl-c-interrupt' $ok "(sort-blocked=$blocked prompt-lines=$prompts)"
    if (-not $ok) { Screenshot 'case-l'; Write-Host "  dump: $(if (Test-Path $c.dump) { Get-Content $c.dump -Raw -Encoding UTF8 } else { '<missing>' })" }
    Stop-Case $c.proc
}

Write-Host "==== e2e summary: PASS=$PASS FAIL=$FAIL ===="
if ($FAIL -gt 0) { exit 1 }
