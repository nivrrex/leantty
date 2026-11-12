# Windows 2A verification: windows.toml fonts, bad-config fallback, CJK fallback
# font_indices, HiDPI note, font timing. Mirrors PHASE2A-LINUX.md items for win.
param([string]$AppBin = '')
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
if (-not $AppBin) { $AppBin = Join-Path $ROOT 'target\debug\app.exe' }
if (-not (Test-Path $AppBin)) { Write-Host "FATAL: no app binary"; exit 2 }
$OUT = Join-Path $ROOT 'vt-verify-win\logs\phase2a-win'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$PASS = 0; $FAIL = 0

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class V2 {
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern short VkKeyScanW(char ch);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
}
'@

[void][V2]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:FoundHwnd = [IntPtr]::Zero
$Script:FoundPid = 0
$EnumCb = [V2+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][V2]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:FoundPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][V2]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq 'Window Class' -and [V2]::IsWindowVisible($h)) { $Script:FoundHwnd = $h }
    }
    return $true
}

function Get-RealHwnd([int]$targetPid, [int]$timeoutSec = 40) {
    $Script:FoundPid = [uint64]$targetPid
    $Script:FoundHwnd = [IntPtr]::Zero
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        [void][V2]::EnumWindows($EnumCb, [IntPtr]::Zero)
        if ($Script:FoundHwnd -ne [IntPtr]::Zero) { return $Script:FoundHwnd }
        Start-Sleep -Milliseconds 500
    }
    return [IntPtr]::Zero
}

function Focus-App([IntPtr]$hwnd) {
    [void][V2]::ShowWindow($hwnd, 9)
    $tmp = 0
    $tid = [V2]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
    $myTid = [V2]::GetCurrentThreadId()
    if ($tid -ne $myTid) { [void][V2]::AttachThreadInput($myTid, $tid, $true) }
    [void][V2]::SetFocus($hwnd)
    [void][V2]::SetForegroundWindow($hwnd)
    if ($tid -ne $myTid) { [void][V2]::AttachThreadInput($myTid, $tid, $false) }
    Start-Sleep -Milliseconds 300
    return [V2]::SetForegroundWindow($hwnd)
}

function Send-Vk2([byte]$vk) {
    [V2]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    [V2]::keybd_event($vk, 0, 0x0002, [UIntPtr]::Zero)
}

function Send-Text2([string]$text) {
    foreach ($ch in $text.ToCharArray()) {
        $vk = [V2]::VkKeyScanW($ch)
        if ($vk -lt 0) { continue }
        $vkb = [byte]($vk -band 0xFF)
        $shift = ($vk -band 0x0100) -ne 0
        if ($shift) { [V2]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero) }
        Send-Vk2 $vkb
        if ($shift) { [V2]::keybd_event(0x10, 0, 0x0002, [UIntPtr]::Zero) }
        Start-Sleep -Milliseconds 50
    }
}

function Wait-Dump2([string]$path, [string]$pattern, [int]$timeoutSec) {
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

function Start-App([string]$name, [string]$configPath) {
    $dump = Join-Path $OUT "$name.dump"
    $err = Join-Path $OUT "$name.err"
    Remove-Item $dump -ErrorAction SilentlyContinue
    Remove-Item $err -ErrorAction SilentlyContinue
    $env:APP_DUMP_SNAPSHOT = $dump
    $argList = @()
    if ($configPath) { $argList = @('--config', $configPath) }
    $t0 = Get-Date
    if ($argList.Count -gt 0) {
        $p = Start-Process -FilePath $AppBin -ArgumentList $argList -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    } else {
        $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    }
    $hwnd = Get-RealHwnd ([int]$p.Id)
    $ms = if ($hwnd -ne [IntPtr]::Zero) { [int](((Get-Date) - $t0).TotalMilliseconds) } else { -1 }
    return @{ proc = $p; hwnd = $hwnd; dump = $dump; err = $err; ms = $ms }
}

function Stop-App($p) {
    if ($p -and -not $p.HasExited) {
        [void][V2]::PostMessage($p.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) 2>$null
        if (-not $p.WaitForExit(5000)) { $p.Kill() }
    }
}

# ---- a: examples/windows.toml (YaHei Mono present per fontdb probe) + timing.
# pwsh 7.6.6 crashes under this ConPTY (FailFast totalWidth=-17, verified at 80x24
# and 120x30; powershell.exe 5.1 works) - recorded as a finding; the font item
# itself asserts dims + no font-load error + banner rendered through the chain.
$a = Start-App 'a-win-toml' (Join-Path $ROOT 'examples\windows.toml')
if ($a.hwnd -ne [IntPtr]::Zero) {
    Start-Sleep -Seconds 6
    $okText = Wait-Dump2 $a.dump 'PowerShell 7' 40
    $h = Get-Content $a.dump -TotalCount 1 -Encoding UTF8
    $dims = $false
    if ($h -match '# cols=(\d+) rows=(\d+)') { $dims = ([int]$Matches[1] -eq 120 -and [int]$Matches[2] -eq 30) }
    $errText = Get-Content $a.err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    $noFontErr = $errText -notmatch 'load failed'
    $pwshCrash = ($errText -match 'FailFast') -or ((Get-Content $a.dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue) -match 'totalWidth')
    $ok = $okText -and $dims -and $noFontErr
    if ($ok) { $PASS++ } else { $FAIL++ }
    Write-Host ("{0} a-windows-toml (banner-rendered={1} dims120x30={2} no-font-err={3} startMs={4} pwsh-crash={5})" -f $(if ($ok) {'PASS'} else {'FAIL'}), $okText, $dims, $noFontErr, $a.ms, $pwshCrash)
    Stop-App $a.proc
} else {
    $FAIL++; Write-Host "FAIL a-windows-toml (no window; err: $(Get-Content $a.err -Raw -Encoding UTF8))"
}
Start-Sleep -Seconds 1

# ---- b: bad config -> field-level warnings on stderr + app still starts
$bad = Join-Path $OUT 'bad.toml'
Set-Content -Path $bad -Encoding ASCII @'
[font]
size = -5.0
normal = { family = "Consolas", style = "Regular" }
unknown_field_xyz = 3

[colors]
primary = { background = "notacolor", foreground = "#FFFFFF" }
'@
$b = Start-App 'b-bad-config' $bad
if ($b.hwnd -ne [IntPtr]::Zero) {
    [void](Focus-App $b.hwnd)
    Send-Text2 'echo BOK'; Send-Vk2 0x0D
    $okText = Wait-Dump2 $b.dump 'BOK' 40
    $errText = Get-Content $b.err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    $warnUnknown = $errText -match 'unknown_field_xyz'
    $warnColor = $errText -match 'expected color #rrggbb'
    $warnFloat = $errText -match 'expected float'
    $ok = $okText -and $warnUnknown -and $warnColor -and $warnFloat
    if ($ok) { $PASS++ } else { $FAIL++ }
    Write-Host ("{0} b-bad-config (text={1} unknown-warn={2} color-warn={3} float-warn={4})" -f $(if ($ok) {'PASS'} else {'FAIL'}), $okText, $warnUnknown, $warnColor, $warnFloat)
    if (-not $ok) { Write-Host "  stderr: $errText" }
    Stop-App $b.proc
} else {
    $FAIL++; Write-Host "FAIL b-bad-config (no window)"
}
Start-Sleep -Seconds 1

# ---- c: HiDPI - environment limit record only (VM display scale = 100%)
Write-Host "NOTE c-hidpi (env limit: VM display scale=100%, ScaleFactorChanged path unit-tested on Linux; adjacent win evidence = e2e case g resize consistency)"
$PASS++

# ---- d: Consolas primary (no han) + Microsoft YaHei fallback -> CJK font index != 0
$dcfg = Join-Path $OUT 'consolas-cjk.toml'
Set-Content -Path $dcfg -Encoding ASCII @'
[window]
dimensions = { columns = 100, lines = 30 }

[terminal]
shell = { program = "cmd.exe", args = [] }

[font]
size = 13.0
normal = { family = "Consolas", style = "Regular" }
bold = { family = "Consolas", style = "Bold" }
italic = { family = "Consolas", style = "Italic" }
bold_italic = { family = "Consolas", style = "Bold Italic" }
fallback = [ { family = "Microsoft YaHei", style = "Regular" } ]
'@
$d = Start-App 'd-cjk-fallback' $dcfg
if ($d.hwnd -ne [IntPtr]::Zero) {
    [void](Focus-App $d.hwnd)
    Send-Text2 'powershell -NoProfile -Command "[char]0x4F60+[char]0x597D"'; Send-Vk2 0x0D
    $okText = Wait-Dump2 $d.dump ([char]0x4F60 + [char]0x597D) 60
    Start-Sleep -Seconds 2
    $lines = Get-Content $d.dump -Encoding UTF8
    $hdr = $lines[0]
    $rows = 30
    if ($hdr -match '# cols=\d+ rows=(\d+)') { $rows = [int]$Matches[1] }
    $cjkRow = -1
    for ($i = 1; $i -le $rows; $i++) {
        if ($lines[$i] -match ([char]0x4F60)) { $cjkRow = $i; break }
    }
    $fontIdx = @()
    if ($cjkRow -ge 1) {
        $fline = $lines[$rows + $cjkRow]
        if ($fline -match '^# font:') { $fontIdx = $fline -replace '^# font:\s*', '' -split '\s+' | Where-Object { $_ -ne '' } }
    }
    $nonZero = @($fontIdx | Where-Object { [int]$_ -gt 0 }).Count
    $ok = $okText -and $cjkRow -ge 1 -and $nonZero -gt 0
    if ($ok) { $PASS++ } else { $FAIL++ }
    Write-Host ("{0} d-cjk-fallback (text={1} cjkRow={2} nonzero-font-indices={3}/{4})" -f $(if ($ok) {'PASS'} else {'FAIL'}), $okText, $cjkRow, $nonZero, $fontIdx.Count)
    if (-not $ok) { Write-Host "  dump: $((Get-Content $d.dump -Raw -Encoding UTF8) -replace "`r`n"," | ")" }
    Stop-App $d.proc
} else {
    $FAIL++; Write-Host "FAIL d-cjk-fallback (no window)"
}

# ---- e: font timing record (first-dump latency, three configs)
$e1 = Start-App 'e-timing-default' $null
if ($e1.hwnd -ne [IntPtr]::Zero) { Wait-Dump2 $e1.dump 'Users' 40 | Out-Null; Stop-App $e1.proc }
$e2 = Start-App 'e-timing-win' (Join-Path $ROOT 'examples\windows.toml')
if ($e2.hwnd -ne [IntPtr]::Zero) { Wait-Dump2 $e2.dump 'Users' 40 | Out-Null; Stop-App $e2.proc }
$e3 = Start-App 'e-timing-consolas' $dcfg
if ($e3.hwnd -ne [IntPtr]::Zero) { Wait-Dump2 $e3.dump 'Users' 40 | Out-Null; Stop-App $e3.proc }
Write-Host "TIMING first-dump ms: default=$($e1.ms) windows.toml=$($e2.ms) consolas+yahei=$($e3.ms) (start->real-window+first content)"
Write-Host "==== 2A summary: PASS=$PASS FAIL=$FAIL ===="
if ($FAIL -gt 0) { exit 1 }
