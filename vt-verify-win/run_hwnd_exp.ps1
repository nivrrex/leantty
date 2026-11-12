# Single-factor: replicate Get-MainHwnd exactly; print type/value/guard/interp.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W3 {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
'@
$ROOT = 'C:\dev\tty'
$env:APP_DUMP_SNAPSHOT = 'C:\dev\tty\vt-verify-win\logs\exp.dump'
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath "$ROOT\target\debug\app.exe" -WorkingDirectory $ROOT -PassThru -RedirectStandardError 'C:\dev\tty\vt-verify-win\logs\exp.err'
"p type: $($p.GetType().FullName); p.Id=$($p.Id)"
Start-Sleep -Seconds 5
$proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
"direct MainWindowHandle: $($proc.MainWindowHandle) type=$($proc.MainWindowHandle.GetType().FullName)"
function Get-MainHwnd([int]$pid, [int]$timeoutSec = 40) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        $proc2 = Get-Process -Id $pid -ErrorAction SilentlyContinue
        if ($proc2 -and $proc2.MainWindowHandle -ne [IntPtr]::Zero) { return $proc2.MainWindowHandle }
        Start-Sleep -Milliseconds 500
    }
    return [IntPtr]::Zero
}
$hwnd = Get-MainHwnd ([int]$p.Id)
"hwnd type: $($hwnd.GetType().FullName)"
"hwnd value: [$hwnd]"
"guard(eq Zero): $($hwnd -eq [IntPtr]::Zero)"
"interp: hwnd=[$($hwnd)]"
$ht = @{ ok = $true; proc = $p; hwnd = $hwnd }
"via hashtable: hwnd=[$($ht.hwnd)]"
"foreground now: $([W3]::GetForegroundWindow())"
"setFg: $([W3]::SetForegroundWindow($hwnd)) fgAfter: $([W3]::GetForegroundWindow())"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
