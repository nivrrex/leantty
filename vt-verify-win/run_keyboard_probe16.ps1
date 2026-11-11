# Probe16: Ctrl+C delivery to idle cmd prompt (2nd prompt line) + CJK clipboard Ctrl+V
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WG {
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
}
'@

[void][WG]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$env:APP_TRACE_FILE = "$OUT\probe16.trace"
$env:APP_DUMP_SNAPSHOT = "$OUT\probe16.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:APP_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe16.err"
$Script:TargetPid = [uint64]$p.Id
Start-Sleep -Seconds 4

$Script:Wins = @()
$cb = [WG+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WG]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:TargetPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][WG]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq 'Window Class' -and [WG]::IsWindowVisible($h)) { $Script:Wins += $h }
    }
    return $true
}
[void][WG]::EnumWindows($cb, [IntPtr]::Zero)
$real = $Script:Wins | Select-Object -First 1
"real=$real"
$tmp = 0
$tid = [WG]::GetWindowThreadProcessId($real, [ref]$tmp)
[void][WG]::ShowWindow($real, 9)
$myTid = [WG]::GetCurrentThreadId()
[void][WG]::AttachThreadInput($myTid, $tid, $true)
[void][WG]::SetFocus($real)
[void][WG]::SetForegroundWindow($real)
[void][WG]::AttachThreadInput($myTid, $tid, $false)
Start-Sleep -Milliseconds 400

function Send-VkG([byte]$vk) {
    [WG]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    [WG]::keybd_event($vk, 0, 0x0002, [UIntPtr]::Zero)
}
function Send-CtrlVG([byte]$vk) {
    [WG]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [WG]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    [WG]::keybd_event($vk, 0, 0x0002, [UIntPtr]::Zero)
    [WG]::keybd_event(0x11, 0, 0x0002, [UIntPtr]::Zero)
}

# stage 1: type echo hi + Enter
foreach ($vk in @(0x45,0x43,0x48,0x4F,0x20,0x48,0x49)) { Send-VkG $vk; Start-Sleep -Milliseconds 60 }
Send-VkG 0x0D
Start-Sleep -Seconds 3
"--- dump after echo hi ---"
Get-Content $env:APP_DUMP_SNAPSHOT | Select-Object -First 3
"keydbg=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG').Count)"

# stage 2: Ctrl+C on idle prompt
Send-CtrlVG 0x43
Start-Sleep -Seconds 3
"--- dump after Ctrl+C ---"
Get-Content $env:APP_DUMP_SNAPSHOT | Select-Object -First 5
"keydbg=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG').Count)"
"--- KEYDBG tail ---"
Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG' | Select-Object -Last 6 | ForEach-Object { $_.Line }

# stage 3: CJK clipboard paste
$b64 = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes([string]([char]0x6C49 + [char]0x5B57)))
$cmd = "[void][Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); [System.Windows.Forms.Clipboard]::SetText([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('$b64')))"
$enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($cmd))
Start-Process powershell -ArgumentList @('-NoProfile','-STA','-EncodedCommand',$enc) -Wait -WindowStyle Hidden
Send-CtrlVG 0x56
Start-Sleep -Seconds 3
"--- dump after CJK paste ---"
Get-Content $env:APP_DUMP_SNAPSHOT | Select-Object -First 5
"keydbg=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG').Count)"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
