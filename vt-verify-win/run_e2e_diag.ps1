# Diagnostic: app startup under Start-Process in script context (no e2e assertions).
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-diag'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$env:LEANTTY_DUMP_SNAPSHOT = "$OUT\diag.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath "$ROOT\target\debug\leantty.exe" -PassThru -RedirectStandardError "$OUT\diag.err" -WorkingDirectory $ROOT
"pid=$($p.Id)"
Start-Sleep -Seconds 8
"hasExited=$($p.HasExited)"
"errExists=$(Test-Path "$OUT\diag.err")"
"errContent=[$(Get-Content "$OUT\diag.err" -Raw -ErrorAction SilentlyContinue)]"
"dumpExists=$(Test-Path "$OUT\diag.dump")"
$proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
if ($proc) {
    "mainWindowHandle=$($proc.MainWindowHandle) title=[$($proc.MainWindowTitle)] responding=$($proc.Responding)"
} else {
    "process gone"
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W2 {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
'@
"foregroundNow=$([W2]::GetForegroundWindow())"
if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) {
    "setFg=$([W2]::SetForegroundWindow($proc.MainWindowHandle)) fgAfter=$([W2]::GetForegroundWindow())"
}
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
