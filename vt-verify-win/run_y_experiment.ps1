# Probe20: experiment Y - app launched from a clean parent (schtasks /IT, explorer),
# record parent PID/name + env proof, measure Ctrl+C on ping -t and Start-Sleep.
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\leantty.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WY {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern short VkKeyScanW(char ch);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
}
'@

[void][WY]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:FoundHwnd = [IntPtr]::Zero
$Script:FoundPid = 0
$EnumCb = [WY+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WY]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:FoundPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][WY]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq 'Window Class' -and [WY]::IsWindowVisible($h)) { $Script:FoundHwnd = $h }
    }
    return $true
}

function Get-RealHwnd([int]$targetPid, [int]$timeoutSec = 40) {
    $Script:FoundPid = [uint64]$targetPid
    $Script:FoundHwnd = [IntPtr]::Zero
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        [void][WY]::EnumWindows($EnumCb, [IntPtr]::Zero)
        if ($Script:FoundHwnd -ne [IntPtr]::Zero) { return $Script:FoundHwnd }
        Start-Sleep -Milliseconds 500
    }
    return [IntPtr]::Zero
}

function Focus-App([IntPtr]$hwnd) {
    [void][WY]::ShowWindow($hwnd, 9)
    $tmp = 0
    $tid = [WY]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
    $myTid = [WY]::GetCurrentThreadId()
    if ($tid -ne $myTid) { [void][WY]::AttachThreadInput($myTid, $tid, $true) }
    [void][WY]::SetFocus($hwnd)
    [void][WY]::SetForegroundWindow($hwnd)
    if ($tid -ne $myTid) { [void][WY]::AttachThreadInput($myTid, $tid, $false) }
    Start-Sleep -Milliseconds 300
}

function Send-VkY([byte]$vk) {
    [WY]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    [WY]::keybd_event($vk, 0, 0x0002, [UIntPtr]::Zero)
}

function Send-TextY([string]$text) {
    foreach ($ch in $text.ToCharArray()) {
        $vk = [WY]::VkKeyScanW($ch)
        if ($vk -lt 0) { continue }
        $vkb = [byte]($vk -band 0xFF)
        $shift = ($vk -band 0x0100) -ne 0
        if ($shift) { [WY]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero) }
        Send-VkY $vkb
        if ($shift) { [WY]::keybd_event(0x10, 0, 0x0002, [UIntPtr]::Zero) }
        Start-Sleep -Milliseconds 50
    }
}

function Send-CtrlCY() {
    [WY]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [WY]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
    [WY]::keybd_event(0x43, 0, 0x0002, [UIntPtr]::Zero)
    [WY]::keybd_event(0x11, 0, 0x0002, [UIntPtr]::Zero)
}

function Wait-DumpY([string]$path, [string]$pattern, [int]$timeoutSec) {
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

function Measure-Y([string]$label, [int]$appPid, [string]$dump) {
    $hwnd = Get-RealHwnd $appPid
    if ($hwnd -eq [IntPtr]::Zero) { "Y[$label] NO-WINDOW"; return }
    Focus-App $hwnd
    # ping -t interrupt
    Send-TextY 'ping -t 127.0.0.1'; Send-VkY 0x0D
    $pre = Wait-DumpY $dump 'TTL=' 20
    $mid = (Get-Content $dump -Raw -Encoding UTF8) -match 'TTL=' | Out-Null
    Send-CtrlCY
    Start-Sleep -Seconds 3
    Send-TextY 'echo Y1DONE'; Send-VkY 0x0D
    $m1 = Wait-DumpY $dump 'Y1DONE' 10
    $grid = Get-Content $dump -Raw -Encoding UTF8
    $ttlAfter = ([regex]::Matches($grid, 'TTL=')).Count
    "Y[$label] ping: pre=$pre marker=$m1 ttlAfter=$ttlAfter (ping interrupted = marker True + ttl stopped)"
    # Start-Sleep interrupt
    Send-TextY 'Start-Sleep -Seconds 60'; Send-VkY 0x0D
    $pre2 = Wait-DumpY $dump 'Start-Sleep' 20
    Start-Sleep -Seconds 1
    Send-CtrlCY
    Start-Sleep -Seconds 3
    Send-TextY 'echo Y2DONE'; Send-VkY 0x0D
    $m2 = Wait-DumpY $dump 'Y2DONE' 10
    "Y[$label] sleep: pre=$pre2 marker=$m2 (sleep interrupted = marker True)"
    [void][WY]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    Start-Sleep -Seconds 2
}

$before = @(Get-Process app -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)

# ---- method 1: schtasks /IT interactive one-shot task
$wrapper = "$OUT\y1-wrapper.cmd"
Set-Content -Path $wrapper -Encoding ASCII @'
@echo off
set LEANTTY_DUMP_SNAPSHOT=C:\dev\tty\vt-verify-win\logs\e2e-probe\y1.dump
set SHELL=powershell.exe
C:\dev\tty\target\debug\leantty.exe
'@
Remove-Item "$OUT\y1.dump" -ErrorAction SilentlyContinue
schtasks /create /tn tty-y1 /tr "cmd /c $wrapper" /sc once /st 23:59 /it /f | Out-Null
schtasks /run /tn tty-y1 | Out-Null
$y1pid = 0
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) {
    $cands = @(Get-Process app -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id })
    if ($cands.Count -gt 0) { $y1pid = $cands[0].Id; break }
    Start-Sleep -Milliseconds 500
}
"m1-schtasks: app-pid=$y1pid"
if ($y1pid -gt 0) {
    $ppid = (Get-CimInstance Win32_Process -Filter "ProcessId=$y1pid").ParentProcessId
    $pname = (Get-CimInstance Win32_Process -Filter "ProcessId=$ppid").Name
    "m1 parent: pid=$ppid name=$pname (host-shell pid=$PID)"
    Measure-Y 'schtasks-IT' $y1pid "$OUT\y1.dump"
    schtasks /delete /tn tty-y1 /f | Out-Null
}

# ---- method 2: explorer.exe launches a cmd wrapper (parent chain explorer->cmd->app)
$wrapper2 = "$OUT\y2-wrapper.cmd"
Set-Content -Path $wrapper2 -Encoding ASCII @'
@echo off
set LEANTTY_DUMP_SNAPSHOT=C:\dev\tty\vt-verify-win\logs\e2e-probe\y2.dump
set SHELL=powershell.exe
C:\dev\tty\target\debug\leantty.exe
'@
Remove-Item "$OUT\y2.dump" -ErrorAction SilentlyContinue
$before2 = @(Get-Process app -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
Start-Process explorer.exe -ArgumentList $wrapper2
$y2pid = 0
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) {
    $cands = @(Get-Process app -ErrorAction SilentlyContinue | Where-Object { $before2 -notcontains $_.Id })
    if ($cands.Count -gt 0) { $y2pid = $cands[0].Id; break }
    Start-Sleep -Milliseconds 500
}
"m2-explorer: app-pid=$y2pid"
if ($y2pid -gt 0) {
    $ppid = (Get-CimInstance Win32_Process -Filter "ProcessId=$y2pid").ParentProcessId
    $pname = (Get-CimInstance Win32_Process -Filter "ProcessId=$ppid").Name
    "m2 parent: pid=$ppid name=$pname (host-shell pid=$PID)"
    Measure-Y 'explorer' $y2pid "$OUT\y2.dump"
}
