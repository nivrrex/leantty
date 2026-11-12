# Probe17: (A) Ctrl+C interrupts blocked 'sort' (proves \x03 write); (B) WM_CHAR delivers CJK
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WH {
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
}
'@

[void][WH]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$env:APP_TRACE_FILE = "$OUT\probe17.trace"
$env:APP_DUMP_SNAPSHOT = "$OUT\probe17.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:APP_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe17.err"
$Script:TargetPid = [uint64]$p.Id
Start-Sleep -Seconds 4

$Script:Wins = @()
$cb = [WH+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WH]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:TargetPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][WH]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq 'Window Class' -and [WH]::IsWindowVisible($h)) { $Script:Wins += $h }
    }
    return $true
}
[void][WH]::EnumWindows($cb, [IntPtr]::Zero)
$real = $Script:Wins | Select-Object -First 1
"real=$real"
$tmp = 0
$tid = [WH]::GetWindowThreadProcessId($real, [ref]$tmp)
[void][WH]::ShowWindow($real, 9)
$myTid = [WH]::GetCurrentThreadId()
[void][WH]::AttachThreadInput($myTid, $tid, $true)
[void][WH]::SetFocus($real)
[void][WH]::SetForegroundWindow($real)
[void][WH]::AttachThreadInput($myTid, $tid, $false)
Start-Sleep -Milliseconds 400

function Send-VkH([byte]$vk) {
    [WH]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    [WH]::keybd_event($vk, 0, 0x0002, [UIntPtr]::Zero)
}

# A: sort blocks on stdin; Ctrl+C should terminate it
foreach ($vk in @(0x53,0x4F,0x52,0x54)) { Send-VkH $vk; Start-Sleep -Milliseconds 60 }
Send-VkH 0x0D
Start-Sleep -Seconds 2
"--- dump A1 (sort running) ---"
Get-Content $env:APP_DUMP_SNAPSHOT | Select-Object -First 4
[WH]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
[WH]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
[WH]::keybd_event(0x43, 0, 0x0002, [UIntPtr]::Zero)
[WH]::keybd_event(0x11, 0, 0x0002, [UIntPtr]::Zero)
Start-Sleep -Seconds 3
"--- dump A2 (after Ctrl+C; expect new prompt line) ---"
Get-Content $env:APP_DUMP_SNAPSHOT | Select-Object -First 6
"keydbg=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG').Count)"

# B: WM_CHAR CJK directly to the winit window
[void][WH]::PostMessage($real, 0x0102, [IntPtr]([uint16]0x6C49), [IntPtr]1)
Start-Sleep -Milliseconds 200
[void][WH]::PostMessage($real, 0x0102, [IntPtr]([uint16]0x5B57), [IntPtr]1)
Start-Sleep -Milliseconds 200
[void][WH]::PostMessage($real, 0x0102, [IntPtr]([uint16]0x000D), [IntPtr]1)
Start-Sleep -Seconds 3
"--- dump B (after WM_CHAR CJK + Enter; expect CJK echoed) ---"
Get-Content $env:APP_DUMP_SNAPSHOT | Select-Object -First 8
"keydbg=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG').Count)"
"--- KEYDBG tail ---"
Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG' | Select-Object -Last 6 | ForEach-Object { $_.Line }
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
