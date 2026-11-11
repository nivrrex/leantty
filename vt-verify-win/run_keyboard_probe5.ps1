# Probe5: VK-key injection (no unicode flag) - does winit/app receive real VK keys?
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W7 {
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern short MapVirtualKey(uint uCode, uint uMapType);
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

[void][W7]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object W7+INPUT))

function Focus-App([IntPtr]$h) {
    [void][W7]::ShowWindow($h, 9)
    [W7]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [W7]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    $tmp = 0
    $appTid = [W7]::GetWindowThreadProcessId($h, [ref]$tmp)
    $myTid = [W7]::GetCurrentThreadId()
    if ($appTid -ne $myTid) { [void][W7]::AttachThreadInput($myTid, $appTid, $true) }
    [void][W7]::SetFocus($h)
    $r = [W7]::SetForegroundWindow($h)
    if ($appTid -ne $myTid) { [void][W7]::AttachThreadInput($myTid, $appTid, $false) }
    return $r
}

function Send-Vk([uint32]$vk) {
    $n = 2
    $inputs = New-Object W7+INPUT[] $n
    $inputs[0] = New-Object W7+INPUT
    $inputs[0].type = 1
    $inputs[0].u.ki.wVk = [uint16]$vk
    $inputs[0].u.ki.wScan = [uint16]([W7]::MapVirtualKey($vk, 0))
    $inputs[0].u.ki.dwFlags = 0
    $inputs[1] = New-Object W7+INPUT
    $inputs[1].type = 1
    $inputs[1].u.ki.wVk = [uint16]$vk
    $inputs[1].u.ki.wScan = [uint16]([W7]::MapVirtualKey($vk, 0))
    $inputs[1].u.ki.dwFlags = 0x0002
    return [uint32][W7]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}

$env:APP_DUMP_SNAPSHOT = "$OUT\probe5.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe5.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"
$ok = $false
for ($i = 1; $i -le 5 -and -not $ok; $i++) {
    $r = Focus-App $hwnd
    Start-Sleep -Milliseconds 300
    $fg = [W7]::GetForegroundWindow()
    "attempt=$i setFg=$r fg=$fg match=$($fg -eq $hwnd)"
    if ($fg -eq $hwnd) { $ok = $true }
}
"focused=$ok"

# VK injection of "echo hello" + Enter
$text = 'echo hello'
foreach ($ch in $text.ToCharArray()) {
    $vk = [uint32][byte][char]::ToUpper($ch)
    if ($ch -eq ' ') { $vk = 0x20 }
    $s = Send-Vk $vk
    "vk=0x{0:X} sent=$s" -f $vk
    Start-Sleep -Milliseconds 60
}
$s = Send-Vk 0x0D
"Enter sent=$s"

for ($k = 1; $k -le 3; $k++) {
    Start-Sleep -Seconds 2
    $fg = [W7]::GetForegroundWindow()
    "t=${k}x2s fg=$fg match=$($fg -eq $hwnd)"
}
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue
"--- err ---"
Get-Content "$OUT\probe5.err" -ErrorAction SilentlyContinue | Select-Object -First 4
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
