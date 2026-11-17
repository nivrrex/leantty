# Probe9: liveness tests - resize changes dump cols/rows? WM_KEYDOWN? WM_CLOSE exits?
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\leantty.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class WA {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
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
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
}
'@

[void][WA]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object WA+INPUT))

$env:LEANTTY_DUMP_SNAPSHOT = "$OUT\probe9.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe9.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"
Start-Sleep -Seconds 3
"dump-1: $((Get-Content $env:LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 1))"

# TEST 1: resize -> new cols/rows in dump proves message pump alive
[void][WA]::SetWindowPos($hwnd, [IntPtr](-1), 100, 100, 1280, 900, 0x0040)
Start-Sleep -Seconds 2
"dump-2-after-resize: $((Get-Content $env:LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 1))"

# TEST 2: focus + SendInput VK 'e' with proper lParam-free struct
$tmp = 0
$appTid = [WA]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
$myTid = [WA]::GetCurrentThreadId()
[void][WA]::ShowWindow($hwnd, 9)
[WA]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
[WA]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
if ($appTid -ne $myTid) { [void][WA]::AttachThreadInput($myTid, $appTid, $true) }
[void][WA]::SetFocus($hwnd)
[void][WA]::SetForegroundWindow($hwnd)
if ($appTid -ne $myTid) { [void][WA]::AttachThreadInput($myTid, $appTid, $false) }

$n = 2
$inputs = New-Object WA+INPUT[] $n
$inputs[0] = New-Object WA+INPUT
$inputs[0].type = 1
$inputs[0].u.ki.wVk = [uint16]0x45
$inputs[0].u.ki.dwFlags = 0
$inputs[1] = New-Object WA+INPUT
$inputs[1].type = 1
$inputs[1].u.ki.wVk = [uint16]0x45
$inputs[1].u.ki.dwFlags = 0x0002
$s = [uint32][WA]::SendInput($n, $inputs, $Script:INPUT_SIZE)
"sendinput-e=$s"
Start-Sleep -Seconds 3
"dump-3-after-key-e: $((Get-Content $env:LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 1))"

# TEST 3: WM_CLOSE exits the app?
[void][WA]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
Start-Sleep -Seconds 6
"alive-after-wmclose=$(-not $p.HasExited)"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
"--- err tail ---"
Get-Content "$OUT\probe9.err" -ErrorAction SilentlyContinue | Select-Object -Last 5
