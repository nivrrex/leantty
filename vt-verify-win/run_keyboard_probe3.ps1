# Probe3: SPI_SETFOREGROUNDLOCKTIMEOUT=0 + AttachThreadInput(app thread) + SetFocus + retry loop
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W5 {
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

$SPI_SETFOREGROUNDLOCKTIMEOUT = 0x2001
[void][W5]::SystemParametersInfo($SPI_SETFOREGROUNDLOCKTIMEOUT, 0, [IntPtr]::Zero, 0)
"spi_set=$?"

$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object W5+INPUT))
"input_size=$Script:INPUT_SIZE"

function Focus-App([IntPtr]$h) {
    [void][W5]::ShowWindow($h, 9)
    [W5]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [W5]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    $myTid = [W5]::GetCurrentThreadId()
    $pidOut = 0
    $appTid = [W5]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ($appTid -ne $myTid) { [void][W5]::AttachThreadInput($myTid, $appTid, $true) }
    [void][W5]::SetFocus($h)
    $r = [W5]::SetForegroundWindow($h)
    if ($appTid -ne $myTid) { [void][W5]::AttachThreadInput($myTid, $appTid, $false) }
    return $r
}

$env:APP_DUMP_SNAPSHOT = "$OUT\probe3.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe3.err"
"pid=$($p.Id)"
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
    $fg = [W5]::GetForegroundWindow()
    "attempt=$i setFg=$r fg=$fg match=$($fg -eq $hwnd)"
    if ($fg -eq $hwnd) { $ok = $true }
}
"focused=$ok"

function Send-Key([ushort]$scan, [uint]$flags) {
    $n = 2
    $inputs = New-Object W5+INPUT[] $n
    $inputs[0] = New-Object W5+INPUT
    $inputs[0].type = 1
    $inputs[0].u.ki.wVk = 0
    $inputs[0].u.ki.wScan = $scan
    $inputs[0].u.ki.dwFlags = $flags
    $inputs[1] = New-Object W5+INPUT
    $inputs[1].type = 1
    $inputs[1].u.ki.wVk = 0
    $inputs[1].u.ki.wScan = $scan
    $inputs[1].u.ki.dwFlags = $flags -bor 0x0002
    [uint32][W5]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}

foreach ($ch in 'echo hello'.ToCharArray()) { [void](Send-Key ([uint16][char]$ch) 0x0004) }
[void](Send-Key ([uint16][char]"`r") 0x0004)
Start-Sleep -Seconds 8
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 8
"fg_final=$([W5]::GetForegroundWindow()) hwnd=$hwnd"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
