# Probe_exit: measure app exit latency after typing 'exit' (f-case mechanism:
# W5 data says ConPTY master reader EOF can lag ~83s when child exits first)
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WE2 {
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
}
'@

[void][WE2]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$env:APP_DUMP_SNAPSHOT = "$OUT\exitprobe.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\exitprobe.err"
$Script:TargetPid = [uint64]$p.Id
Start-Sleep -Seconds 4
$Script:Wins = @()
$cb = [WE2+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WE2]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:TargetPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][WE2]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq 'Window Class' -and [WE2]::IsWindowVisible($h)) { $Script:Wins += $h }
    }
    return $true
}
[void][WE2]::EnumWindows($cb, [IntPtr]::Zero)
$real = $Script:Wins | Select-Object -First 1
"real=$real"
$tmp = 0
$tid = [WE2]::GetWindowThreadProcessId($real, [ref]$tmp)
[void][WE2]::ShowWindow($real, 9)
$myTid = [WE2]::GetCurrentThreadId()
[void][WE2]::AttachThreadInput($myTid, $tid, $true)
[void][WE2]::SetFocus($real)
[void][WE2]::SetForegroundWindow($real)
[void][WE2]::AttachThreadInput($myTid, $tid, $false)
Start-Sleep -Milliseconds 400

foreach ($vk in @(0x45,0x58,0x49,0x54)) {  # e x i t
    [WE2]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero)
    [WE2]::keybd_event([byte]$vk, 0, 0x0002, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
}
[WE2]::keybd_event(0x0D, 0, 0, [UIntPtr]::Zero)
[WE2]::keybd_event(0x0D, 0, 0x0002, [UIntPtr]::Zero)
"exit-typed-at=$(((Get-Date).ToString('HH:mm:ss')))"
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 100) {
    if ($p.HasExited) { "app-exited-after=$([int]((Get-Date) - $t0).TotalSeconds)s code=$($p.ExitCode)"; break }
    Start-Sleep -Seconds 2
}
if (-not $p.HasExited) { "app-still-alive-at-100s"; Stop-Process -Id $p.Id -Force }
"--- dump tail ---"
Get-Content $env:APP_DUMP_SNAPSHOT -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -First 4
