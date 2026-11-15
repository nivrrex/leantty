# run-tonight.ps1 - single entry point of the "tonight kit v1" (no GPU build).
#
# Rules (also in README-TONIGHT.md):
#   * only reads/writes inside this kit directory, no network, no registry writes,
#     no system settings, no firewall rules, no global environment changes
#   * all paths are derived from $PSScriptRoot
#   * only the app.exe started by this script is stopped (by PID); OpenConsole.exe,
#     conhost.exe, wezterm, pwsh processes are never touched
#   * every stage has its own time limit and its own try/catch: one failure never
#     stops the remaining stages
#   * perf logs use a DIRECT file handle (wrapper .cmd with `2>log`) and
#     APP_PERF_FRAMES=1; PowerShell's Start-Process redirection is never used
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File run-tonight.ps1
#   powershell -ExecutionPolicy Bypass -File run-tonight.ps1 -Quick
#   powershell -ExecutionPolicy Bypass -File run-tonight.ps1 -SoakMinutes 10 -OutDir .\results
# ASCII only.
param(
    [int]$SoakMinutes = 5,
    [switch]$SkipSoak,
    [string]$OutDir = '',
    [switch]$Quick
)
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
. (Join-Path $Root 'lib.ps1')

$StartTime = Get-Date
if (-not $OutDir) { $OutDir = Join-Path $Root 'results' }
if (-not [IO.Path]::IsPathRooted($OutDir)) { $OutDir = Join-Path $Root $OutDir }
$Stamp = $StartTime.ToString('yyyyMMdd-HHmmss')
$RunDir = Join-Path $OutDir ('tonight-' + $Stamp)
New-Item -ItemType Directory -Force -Path $RunDir | Out-Null
$ShotDir = Join-Path $RunDir 'shots'
$LogDir = Join-Path $RunDir 'logs'
New-Item -ItemType Directory -Force -Path $ShotDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

$App = Join-Path $Root 'app.exe'
$CfgCpu = Join-Path $Root 'configs\cpu.toml'
$CfgGpu = Join-Path $Root 'configs\gpu.toml'
$CfgGpuAuto = Join-Path $Root 'configs\gpu-auto.toml'
if ($Quick) {
    $SoakMinutes = [math]::Max(1, [math]::Min($SoakMinutes, 1))
}
$script:Results = @()          # stage records
$script:Notes = @()            # free-form notes for summary
$script:Env = @{}
$script:Rdp = $false
$script:GpuAvailable = $false
$script:FirstFrame = @{}       # backend -> @(values)
$script:ShotCpu = $null
$script:Deadline = $null       # per-stage deadline (DateTime)

function Test-Deadline {
    if ($script:Deadline -and (Get-Date) -gt $script:Deadline) {
        throw ('stage time limit exceeded ({0})' -f $script:Deadline.ToString('HH:mm:ss'))
    }
}

function Invoke-Stage {
    param([string]$Name, [int]$LimitSec, [string]$Title, [scriptblock]$Body)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $script:Deadline = (Get-Date).AddSeconds($LimitSec)
    $status = 'PASS'; $errorText = ''
    $script:StageStatusOverride = $null
    $script:StageNote = ''
    Write-Host ('== {0} {1} (limit {2}s) ==' -f $Name, $Title, $LimitSec)
    try {
        & $Body
    } catch {
        $status = 'FAIL'
        $errorText = $_.Exception.Message
        Write-Host ('!! {0} failed: {1}' -f $Name, $errorText)
    } finally {
        $sw.Stop()
        $script:Deadline = $null
        if ($script:StageStatusOverride -and $status -eq 'PASS') { $status = $script:StageStatusOverride }
        $script:Results += [pscustomobject]@{
            stage = $Name
            title = $Title
            status = $status
            seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            error = $(if ($errorText) { $errorText } elseif ($script:StageNote) { $script:StageNote } else { '' })
        }
        Write-Host ('-- {0}: {1} in {2:N1}s' -f $Name, $status, $sw.Elapsed.TotalSeconds)
    }
}

function New-AppShot([string]$Tag, [string]$Config, [string]$LogTag) {
    $log = Join-Path $LogDir ('app-{0}.log' -f $LogTag)
    $h = Start-AppDirect -Exe $App -Config $Config -LogPath $log -WorkDir $RunDir
    if (-not $h.App) { Stop-OwnApp $h.App $h.Cmd; throw ('app.exe did not start (log {0})' -f $log) }
    $hwnd = Wait-AppWindow -App $h.App -TimeoutMs 20000
    if ($hwnd -eq [IntPtr]::Zero) { throw 'app window did not appear' }
    return [pscustomobject]@{ Handle = $h; Hwnd = $hwnd; Log = $log; Tag = $Tag }
}

function Close-Session($Session) {
    if ($Session -and $Session.Handle) { Stop-OwnApp $Session.Handle.App $Session.Handle.Cmd }
}

function Start-Session([string]$Config, [string]$LogTag) {
    $log = Join-Path $LogDir ('app-{0}.log' -f $LogTag)
    $h = Start-AppDirect -Exe $App -Config $Config -LogPath $log -WorkDir $Root
    if (-not $h.App) { Stop-OwnApp $h.App $h.Cmd; throw ('app.exe did not start (log {0})' -f $log) }
    $hwnd = Wait-AppWindow -App $h.App -TimeoutMs 20000
    if ($hwnd -eq [IntPtr]::Zero) { Stop-OwnApp $h.App $h.Cmd; throw 'app window did not appear' }
    $focus = Focus-Window $hwnd
    Start-Sleep -Milliseconds 700
    return [pscustomobject]@{ Handle = $h; Hwnd = $hwnd; Log = $log; FocusOk = $focus }
}

function Set-AppFocus {
    # click into the window as well: after the cover-window test SetForegroundWindow
    # alone sometimes leaves the focus with another window
    param([IntPtr]$Hwnd)
    for ($i = 1; $i -le 3; $i++) {
        [void](Focus-Window $Hwnd)
        $g = Get-Geometry $Hwnd
        Click-Button ([int]($g.x + $g.w / 2)) ([int]($g.y + $g.h / 2)) 0x0002 0x0004
        Start-Sleep -Milliseconds 300
        if ([TN.Win]::GetForegroundWindow() -eq $Hwnd) { return $true }
    }
    return $false
}

function Confirm-Typing {
    # types a tiny command that creates a marker file; retries once
    param($Session, [string]$Command, [string]$Marker, [int]$TimeoutMs = 8000)
    if (Test-Path $Marker) { Remove-Item $Marker -Force }
    for ($i = 1; $i -le 2; $i++) {
        [void](Set-AppFocus $Session.Hwnd)
        Send-Line $Command
        if (Wait-File $Marker $TimeoutMs) { return $true }
    }
    return $false
}

function Wait-PerfKey([string]$Log, [string]$Key, [int]$TimeoutMs = 20000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        $v = Get-PerfValue $Log $Key
        if ($null -ne $v) { return $v }
        Start-Sleep -Milliseconds 100
    }
    return $null
}

# ---------------------------------------------------------------- P0 environment
Invoke-Stage -Name 'P0' -LimitSec 240 -Title 'environment capture' -Body {
    $os = Get-CimInstance Win32_OperatingSystem
    $cs = Get-CimInstance Win32_ComputerSystem
    $cpu = @(Get-CimInstance Win32_Processor)
    $script:Env.os = ('{0} {1} build {2}' -f $os.Caption, $os.Version, $os.BuildNumber)
    $script:Env.computer = $cs.Name
    $script:Env.cpu = ($cpu | ForEach-Object { '{0} ({1}C/{2}T)' -f $_.Name.Trim(), $_.NumberOfCores, $_.NumberOfLogicalProcessors }) -join '; '
    $script:Env.ram_gb = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
    $script:Env.ps_version = $PSVersionTable.PSVersion.ToString()
    $script:Env.timestamp = $StartTime.ToString('o')
    $script:Env.quick = [bool]$Quick
    $script:Env.soak_minutes = $SoakMinutes

    $vc = @(Get-CimInstance Win32_VideoController)
    $script:Env.video = @()
    foreach ($v in $vc) {
        $script:Env.video += [pscustomobject]@{
            name = $v.Name
            driver_version = $v.DriverVersion
            adapter_ram_mb = if ($v.AdapterRAM) { [math]::Round($v.AdapterRAM / 1MB) } else { 0 }
            video_processor = $v.VideoProcessor
            pnp_device_id = $v.PNPDeviceID
            current_mode = ('{0}x{1}' -f $v.CurrentHorizontalResolution, $v.CurrentVerticalResolution)
        }
    }

    $script:Env.displays = @(Get-Displays)
    $script:Env.dpi_aware = [bool](Initialize-DpiAware)
    $script:Rdp = Test-RdpSession
    $script:Env.rdp = [bool]$script:Rdp
    $script:Env.sessionname = $env:SESSIONNAME

    # read-only registry enumeration: does a Vulkan ICD exist?
    $vk = @()
    try {
        $p = 'HKLM:\SOFTWARE\Khronos\Vulkan\Drivers'
        if (Test-Path $p) {
            $item = Get-Item $p
            $vk = @($item.GetValueNames())
        }
    } catch { }
    $script:Env.vulkan_icd = $vk
    $script:Env.vulkan_icd_count = $vk.Count

    # dxdiag with a timeout; we started it, so stopping it by PID is allowed
    $dxPath = Join-Path $RunDir 'dxdiag.txt'
    try {
        $dx = Start-Process -FilePath 'dxdiag.exe' -ArgumentList @('/whql:off', '/t', $dxPath) -PassThru
        if (-not $dx.WaitForExit(90000)) {
            Stop-Process -Id $dx.Id -ErrorAction SilentlyContinue
            $script:Notes += 'dxdiag timed out (90 s); the dxdiag process started here was stopped by PID, no dxdiag.txt produced'
        } else {
            $script:Notes += ('dxdiag exit code {0}' -f $dx.ExitCode)
        }
    } catch {
        $script:Notes += ('dxdiag failed: {0}' -f $_.Exception.Message)
    }

    $fi = Get-Item $App
    $script:Env.app_sha256 = Get-Sha256 $App
    $script:Env.app_bytes = $fi.Length
    $script:Env.app_version = (Get-Item $App).VersionInfo.FileVersion
    $script:Env.exe_path = $App
    $script:Env | ConvertTo-Json -Depth 6 | Set-Content -Path (Join-Path $RunDir 'environment.json') -Encoding UTF8
    return
}

# ---------------------------------------------------------------- P1 CPU baseline
Invoke-Stage -Name 'P1' -LimitSec 900 -Title 'CPU baseline' -Body {
    # 1) five cold starts -> first_frame_ms
    $vals = @()
    for ($i = 1; $i -le 5; $i++) {
        Test-Deadline
        $s = Start-Session $CfgCpu ('p1-start-{0}' -f $i)
        $ff = Wait-PerfKey $s.Log 'first_frame_ms' 25000
        if ($null -ne $ff) { $vals += $ff }
        Close-Session $s
        Start-Sleep -Milliseconds 900
    }
    if ($vals.Count -eq 0) { throw 'no first_frame_ms captured in 5 starts' }
    $script:FirstFrame['cpu'] = $vals
    $script:Notes += ('P1 first_frame_ms median {0:N0} / min {1:N0} (n={2})' -f (($vals | Measure-Object -Average).Average), ($vals | Measure-Object -Minimum).Minimum, $vals.Count)

    # 2) fixed content via injected script (marker file proves the injection)
    # short, space-free command name: long typed paths are fragile (and the first
    # synthetic keys are sometimes swallowed while the window takes focus)
    $sample = Join-Path $Root 'sample.ps1'
    $marker = Join-Path $Root 'sample-done.txt'
    if (Test-Path $marker) { Remove-Item $marker -Force }
    $body = @(
        '$ErrorActionPreference = "Continue"',
        # relative path only: the shell runs in the kit directory, and a Chinese
        # path inside an ASCII-encoded .ps1 would be mangled
        '$out = New-Object System.IO.StreamWriter("sample-done.txt", $false, (New-Object System.Text.UTF8Encoding($false)))',
        '$out.WriteLine("ascii : ABC abc 0123 ~!@#$%^&*()")',
        ('$out.WriteLine("cjk   : " + [string][char]0x4E2D + [string][char]0x6587 + [string][char]0x6D4B + [string][char]0x8BD5)'),
        '$out.WriteLine("wide  : " + [string][char]0xFF21 + [string][char]0xFF22 + [string][char]0xFF23)',
        '$out.Close()',
        'Write-Host ""',
        'Write-Host "ascii : ABC abc 0123 ~!@#$%^&*()"',
        ('Write-Host ("cjk   : " + [string][char]0x4E2D + [string][char]0x6587 + [string][char]0x6D4B + [string][char]0x8BD5)'),
        ('Write-Host ("wide  : " + [string][char]0xFF21 + [string][char]0xFF22 + [string][char]0xFF23)'),
        'Write-Host "colors:" -NoNewline',
        'for ($i = 31; $i -le 37; $i++) { Write-Host ("  " + [char]27 + "[" + $i + "mXX" + [char]27 + "[0m") -NoNewline }',
        'Write-Host ""',
        ('Write-Host ([char]27 + "[7mINVERSE-LINE-" + [char]27 + "[0m normal)")'),
        'Write-Host "END-OF-SAMPLE"'
    ) -join "`r`n"
    $body | Set-Content -Path $sample -Encoding ASCII -NoNewline
    # cmd.exe resolves "sample" to sample.cmd in the working directory: six
    # keystrokes instead of a long path
    ('@echo off' + "`r`n" + 'powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0sample.ps1"') |
        Set-Content -Path (Join-Path $Root 'sample.cmd') -Encoding ASCII

    $s = Start-Session $CfgCpu 'p1-content'
    try {
        if (-not $s.FocusOk) { $script:Notes += 'P1: window did not reach the foreground; rely on the marker file for injection validity' }
        # the shell starts in the user profile, so switch to the kit directory
        # through its 8.3 name (ASCII, no spaces, no Chinese characters)
        $short = Get-ShortPath $Root
        Send-Line ('cd /d ' + $short)
        Start-Sleep -Milliseconds 600
        Send-Line 'sample'
        if (-not (Wait-File $marker 20000)) {
            # one retry: the first synthetic keys can be swallowed while the
            # window takes focus (observed in earlier phases)
            Send-Line 'sample'
            if (-not (Wait-File $marker 20000)) { throw 'P1 injection did not take effect: marker file sample-done.txt never appeared' }
        }
        Start-Sleep -Seconds 2
        $shots = 0
        $p = Join-Path $ShotDir 'S_cpu.png'
        for ($k = 0; $k -lt 3; $k++) {
            [void](Focus-Window $s.Hwnd)
            Start-Sleep -Milliseconds 400
            try { [void](Save-Shot $s.Hwnd $p); $shots++; break } catch { Start-Sleep -Milliseconds 600 }
        }
        if ($shots -eq 0) { throw 'S_cpu.png screenshot failed' }
        $script:ShotCpu = $p
        $black = Get-BlackRatio $p
        if ($black -gt 0.9) { throw ('S_cpu.png is almost entirely black (black ratio {0})' -f $black) }

        # 3) idle memory for 30 s
        $mem = @()
        foreach ($t in 5, 15, 30) {
            Test-Deadline
            Start-Sleep -Seconds ($t - ($(if ($mem.Count) { $mem[-1].t_s } else { 0 })))
            $st = Get-AppStats $s.Handle.App.Id
            if ($st) { $mem += [pscustomobject]@{ t_s = $t; ws_mb = $st.ws_mb; priv_mb = $st.priv_mb; handles = $st.handles; threads = $st.threads } }
        }
        $script:Env.idle_cpu = $mem
        $mem | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $RunDir 'idle-cpu.json') -Encoding UTF8

        # 4) resize 20 times, then verify the picture and the marker
        $g = Get-Geometry $s.Hwnd
        $w1 = $g.w; $h1 = $g.h
        [void](Set-AppFocus $s.Hwnd)
        Start-Sleep -Milliseconds 500
        $preResize = Join-Path $ShotDir 'S_cpu_before_resize.png'
        [void](Save-Shot $s.Hwnd $preResize)
        for ($i = 0; $i -lt 20; $i++) {
            Test-Deadline
            $w = if ($i % 2 -eq 0) { $w1 - 160 } else { $w1 }
            $h = if ($i % 2 -eq 0) { $h1 - 100 } else { $h1 }
            [void][TN.Win]::SetWindowPos($s.Hwnd, [IntPtr]::Zero, $g.x, $g.y, $w, $h, 0x0004)
            Start-Sleep -Milliseconds 250
        }
        [void][TN.Win]::SetWindowPos($s.Hwnd, [IntPtr]::Zero, $g.x, $g.y, $w1, $h1, 0x0004)
        Start-Sleep -Milliseconds 800
        [void](Focus-Window $s.Hwnd)
        Start-Sleep -Milliseconds 400
        $after = Join-Path $ShotDir 'S_cpu_after_resize.png'
        [void](Save-Shot $s.Hwnd $after)
        $cmp = Compare-Png $preResize $after
        $script:Env.resize_cmp = $cmp
        if (-not $cmp.same_size) { $script:Notes += 'P1: screenshot size after resize differs from the baseline (window restore was not exact)' }

        # 5) minimize/restore and cover/uncover -> screenshots must match
        [void](Set-AppFocus $s.Hwnd)
        Start-Sleep -Milliseconds 400
        $preMin = Join-Path $ShotDir 'S_cpu_before_minimize.png'
        [void](Save-Shot $s.Hwnd $preMin)
        [void][TN.Win]::ShowWindow($s.Hwnd, 6)      # SW_MINIMIZE
        Start-Sleep -Milliseconds 900
        [void][TN.Win]::ShowWindow($s.Hwnd, 9)      # SW_RESTORE
        [void](Focus-Window $s.Hwnd)
        Start-Sleep -Milliseconds 900
        $minShot = Join-Path $ShotDir 'S_cpu_after_minimize.png'
        [void](Save-Shot $s.Hwnd $minShot)
        $cmpMin = Compare-Png $preMin $minShot
        $script:Env.minimize_cmp = $cmpMin

        Add-Type -AssemblyName System.Windows.Forms
        $cover = New-Object System.Windows.Forms.Form
        $cover.Text = 'tonight-cover'
        $cover.FormBorderStyle = 'None'
        $cover.StartPosition = 'Manual'
        $cover.Size = New-Object System.Drawing.Size ($g.w + 40), ($g.h + 40)
        $cover.Location = New-Object System.Drawing.Point ([math]::Max(0, $g.x - 20)), ([math]::Max(0, $g.y - 20))
        $cover.TopMost = $true
        $cover.Show()
        [System.Windows.Forms.Application]::DoEvents()
        Start-Sleep -Milliseconds 1200
        $cover.Close()
        $cover.Dispose()
        [void](Focus-Window $s.Hwnd)
        Start-Sleep -Milliseconds 900
        $coverShot = Join-Path $ShotDir 'S_cpu_after_cover.png'
        [void](Save-Shot $s.Hwnd $coverShot)
        $cmpCover = Compare-Png $minShot $coverShot
        $script:Env.cover_cmp = $cmpCover

        # 6) throughput inputs are generated here; the 120x30 and 200x60 runs use
        #    their own fresh sessions (a session that went through resize/minimize/
        #    cover tests is not a clean throughput subject)
        $big = Join-Path $Root 'big.txt'
        $bytes = New-TestFile -Path $big -Lines 300000
        $script:Notes += ('P1 throughput input {0:N1} MB, 300000 lines' -f ($bytes / 1MB))
        $cfg200 = Join-Path $Root 'cpu-200x60.toml'
        $txt = Get-Content $CfgCpu -Raw -Encoding UTF8
        $txt = $txt -replace 'dimensions = \{ columns = \d+, lines = \d+ \}', 'dimensions = { columns = 200, lines = 60 }'
        $txt | Set-Content -Path $cfg200 -Encoding UTF8
    } finally {
        Close-Session $s
    }

    # 7) throughput 120x30 in a fresh session
    $s3 = Start-Session $CfgCpu 'p1-tp120'
    try {
        $tp1 = Join-Path $Root 'tp120.done'
        if (Test-Path $tp1) { Remove-Item $tp1 -Force }
        [void](Set-AppFocus $s3.Hwnd)
        Send-Line ('cd /d ' + (Get-ShortPath $Root))
        Start-Sleep -Milliseconds 600
        $ready1 = Join-Path $Root 'tp-ready1.done'
        if (-not (Confirm-Typing $s3 'echo READY>tp-ready1.done' $ready1 8000)) {
            throw 'P1: typing into the app did not take effect (tp-ready1.done missing)'
        }
        [void](Set-AppFocus $s3.Hwnd)
        $t0 = Get-Date
        Send-Line 'type big.txt & echo DONE>tp120.done'
        $remain = [int](($script:Deadline - (Get-Date)).TotalSeconds)
        $ok = Wait-File $tp1 ([math]::Min(900, [math]::Max(60, $remain)) * 1000)
        $el = ((Get-Date) - $t0).TotalSeconds
        if (-not $ok) { throw ('P1 throughput did not finish in time (120x30, after {0:N0}s)' -f $el) }
        Start-Sleep -Seconds 3
        $frames = Get-PerfFrames $s3.Log
        $script:Env.tp_120x30 = [pscustomobject]@{ seconds = [math]::Round($el, 1); frames = $frames }
        $script:Notes += ('P1 throughput 120x30: {0:N1}s for {1:N1} MB' -f $el, ($bytes / 1MB))
    } finally {
        Close-Session $s3
    }

    $s2 = Start-Session (Join-Path $Root 'cpu-200x60.toml') 'p1-tp200'
    try {
        $tp2 = Join-Path $Root 'tp200.done'
        if (Test-Path $tp2) { Remove-Item $tp2 -Force }
        [void](Set-AppFocus $s2.Hwnd)
        Send-Line ('cd /d ' + (Get-ShortPath $Root))
        Start-Sleep -Milliseconds 600
        $ready2 = Join-Path $Root 'tp-ready2.done'
        if (-not (Confirm-Typing $s2 'echo READY>tp-ready2.done' $ready2 8000)) {
            throw 'P1: typing into the app did not take effect (tp-ready2.done missing)'
        }
        [void](Set-AppFocus $s2.Hwnd)
        $t0 = Get-Date
        Send-Line 'type big.txt & echo DONE>tp200.done'
        $remain2 = [int](($script:Deadline - (Get-Date)).TotalSeconds)
        $ok = Wait-File $tp2 ([math]::Min(900, [math]::Max(60, $remain2)) * 1000)
        $el = ((Get-Date) - $t0).TotalSeconds
        if (-not $ok) { throw ('P1 throughput did not finish in time (200x60, after {0:N0}s)' -f $el) }
        Start-Sleep -Seconds 3
        $frames = Get-PerfFrames $s2.Log
        $script:Env.tp_200x60 = [pscustomobject]@{ seconds = [math]::Round($el, 1); frames = $frames }
    } finally {
        Close-Session $s2
    }
}

# ---------------------------------------------------------------- P2 GPU
Invoke-Stage -Name 'P2' -LimitSec 600 -Title 'GPU backends (N/A when build has no GPU)' -Body {
    if ($script:Rdp) {
        $script:StageStatusOverride = 'SKIP'
        $script:StageNote = 'GPU data is meaningless in an RDP session'
        $script:Notes += 'P2 skipped: remote desktop session, GPU data would be meaningless'
        return
    }
    $variants = @(
        @{ tag = 'default'; env = @{}; cfg = $CfgGpuAuto },
        @{ tag = 'dx12'; env = @{ WGPU_BACKEND = 'dx12' }; cfg = $CfgGpu },
        @{ tag = 'vulkan'; env = @{ WGPU_BACKEND = 'vulkan' }; cfg = $CfgGpu }
    )
    $naReasons = @()
    foreach ($v in $variants) {
        Test-Deadline
        $log = Join-Path $LogDir ('app-p2-{0}.log' -f $v.tag)
        foreach ($k in $v.env.Keys) { Set-Item -Path ('Env:' + $k) -Value $v.env[$k] }
        try {
            $h = Start-AppDirect -Exe $App -Config $v.cfg -LogPath $log -WorkDir $Root
        } finally {
            foreach ($k in $v.env.Keys) { Remove-Item -Path ('Env:' + $k) -ErrorAction SilentlyContinue }
        }
        if (-not $h.App) {
            $naReasons += ('{0}: failed to start' -f $v.tag)
            Stop-OwnApp $h.App $h.Cmd
            continue
        }
        # give the renderer a moment, then decide N/A *before* any window work
        Start-Sleep -Seconds 3
        $txt = ''
        if (Test-Path $log) { try { $txt = Get-Content $log -Raw -Encoding UTF8 -ErrorAction SilentlyContinue } catch { } }
        # a real GPU path is the ONLY thing that prints gpu_init_ms; backend=gpu on
        # a CPU-only build prints the platform-unavailable line and falls back
        $unavailable = -not ($txt -match 'gpu_init_ms')
        if ($unavailable -or $h.App.HasExited) {
            $naReasons += ('{0}: this build has no GPU backend ({1})' -f $v.tag, $(if ($unavailable) { 'stderr reports platform unavailable' } else { 'exited on start' }))
            Stop-OwnApp $h.App $h.Cmd
            continue
        }
        $hwnd = Wait-AppWindow -App $h.App -TimeoutMs 15000
        if ($hwnd -eq [IntPtr]::Zero) {
            $naReasons += ('{0}: no usable window' -f $v.tag)
            Stop-OwnApp $h.App $h.Cmd
            continue
        }
        # a working GPU path: five starts, adapter info, screenshot, idle memory
        $script:GpuAvailable = $true
        $vals = @()
        $adapters = Get-PerfLine $log 'gpu_init_ms'
        for ($i = 1; $i -le 5; $i++) {
            Test-Deadline
            $ff = Wait-PerfKey $log 'first_frame_ms' 20000
            if ($null -ne $ff) { $vals += $ff }
            if ($i -lt 5) {
                Stop-OwnApp $h.App $h.Cmd
                Start-Sleep -Milliseconds 800
                $h = Start-AppDirect -Exe $App -Config $v.cfg -LogPath $log -WorkDir $Root
                $hwnd = Wait-AppWindow -App $h.App -TimeoutMs 15000
            }
        }
        $script:FirstFrame['gpu-' + $v.tag] = $vals
        [void](Focus-Window $hwnd)
        Start-Sleep -Milliseconds 800
        $shot = Join-Path $ShotDir ('S_gpu_{0}.png' -f $v.tag)
        [void](Save-Shot $hwnd $shot)
        if ($script:ShotCpu) {
            $cmp = Compare-Png $script:ShotCpu $shot
            $script:Env['gpu_cmp_' + $v.tag] = $cmp
        }
        $black = Get-BlackRatio $shot
        if ($black -gt 0.9) { $script:Notes += ('P2 {0}: S_gpu_{0}.png is almost entirely black ({1}) -> FAIL' -f $v.tag, $black) }
        $mem = @()
        foreach ($t in 5, 15, 30) {
            Test-Deadline
            Start-Sleep -Seconds 5
            $st = Get-AppStats $h.App.Id
            if ($st) { $mem += [pscustomobject]@{ t_s = $t; ws_mb = $st.ws_mb; priv_mb = $st.priv_mb } }
        }
        $script:Env['idle_gpu_' + $v.tag] = $mem
        $script:Env['gpu_info_' + $v.tag] = (Get-PerfLine $log 'gpu_init_ms')
        Stop-OwnApp $h.App $h.Cmd
    }
    if (-not $script:GpuAvailable) {
        $script:StageStatusOverride = 'N/A'
        $script:StageNote = ($naReasons -join ' | ')
        $script:Notes += ('P2 = N/A: this build has no GPU backend ({0})' -f ($naReasons -join '; '))
    }
}

# ---------------------------------------------------------------- P3 soak
Invoke-Stage -Name 'P3' -LimitSec ([math]::Max(180, $SoakMinutes * 60 + 240)) -Title 'soak' -Body {
    if ($SkipSoak) { $script:Notes += 'P3 skipped: -SkipSoak'; return }
    $backends = @(@{ tag = 'cpu'; cfg = $CfgCpu })
    if ($script:GpuAvailable) { $backends += @{ tag = 'gpu'; cfg = $CfgGpuAuto } }
    $minutes = if ($Quick) { 1 } else { $SoakMinutes }
    foreach ($b in $backends) {
        Test-Deadline
        $s = Start-Session $b.cfg ('p3-' + $b.tag)
        $samples = @()
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $cycle = 0
        try {
            while ($sw.Elapsed.TotalMinutes -lt $minutes) {
                Test-Deadline
                Start-Sleep -Seconds 30
                $cycle++
                $st = Get-AppStats $s.Handle.App.Id
                if ($st) {
                    $samples += [pscustomobject]@{
                        t_s = [math]::Round($sw.Elapsed.TotalSeconds)
                        ws_mb = $st.ws_mb; priv_mb = $st.priv_mb; handles = $st.handles; threads = $st.threads
                    }
                }
                # load: output + resize + (every 3rd cycle) a real-path 1 MB paste
                try {
                    [void](Focus-Window $s.Hwnd)
                    Send-Line ('for /l %i in (1,1,200) do @echo soak-' + $cycle)
                    $g = Get-Geometry $s.Hwnd
                    [void][TN.Win]::SetWindowPos($s.Hwnd, [IntPtr]::Zero, $g.x, $g.y, ($g.w - 100), ($g.h - 60), 0x0004)
                    Start-Sleep -Milliseconds 300
                    [void][TN.Win]::SetWindowPos($s.Hwnd, [IntPtr]::Zero, $g.x, $g.y, $g.w, $g.h, 0x0004)
                } catch { $script:Notes += ('P3 load step failed: {0}' -f $_.Exception.Message) }
                if ($cycle % 3 -eq 0) {
                    try {
                        Add-Type -AssemblyName System.Windows.Forms
                        [System.Windows.Forms.Clipboard]::SetText(('a' * (1024 * 1024)))
                        $g = Get-Geometry $s.Hwnd
                        Click-Button ([int]($g.x + $g.w / 2)) ([int]($g.y + $g.h / 2)) 0x0008 0x0010
                        Start-Sleep -Milliseconds 500
                        $script:Notes += ('P3 {0} cycle {1}: 1 MB paste triggered' -f $b.tag, $cycle)
                    } catch { $script:Notes += ('P3 paste failed: {0}' -f $_.Exception.Message) }
                }
                if (Test-LogHasPanic $s.Log) { throw 'P3: panic found in the app log' }
            }
        } finally {
            $samples | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $RunDir ('soak-{0}.json' -f $b.tag)) -Encoding UTF8
            $script:Env['soak_' + $b.tag] = $samples
            $panicked = Test-LogHasPanic $s.Log
            # close via the window and check that it exits
            $exited = $false
            try {
                [void]$s.Handle.App.CloseMainWindow()
                $sw2 = [Diagnostics.Stopwatch]::StartNew()
                while ($sw2.ElapsedMilliseconds -lt 8000) {
                    $s.Handle.App.Refresh()
                    if ($s.Handle.App.HasExited) { $exited = $true; break }
                    Start-Sleep -Milliseconds 200
                }
            } catch { }
            $script:Env['soak_exit_' + $b.tag] = $exited
            $script:Notes += ('P3 {0}: {1} samples, panic={2}, clean exit after close={3}' -f $b.tag, $samples.Count, $panicked, $exited)
            if ($panicked) { $script:Results += [pscustomobject]@{ stage = 'P3'; title = ('soak ' + $b.tag); status = 'FAIL'; seconds = 0; error = 'panic found in stderr' } }
            Close-Session $s
        }
    }
}

# ---------------------------------------------------------------- P4 summary
Invoke-Stage -Name 'P4' -LimitSec 240 -Title 'summary + zip' -Body {
    $endTime = Get-Date
    $elapsed = ($endTime - $StartTime).TotalMinutes
    $md = New-Object System.Collections.Generic.List[string]
    $md.Add('# tonight kit v1 - run report')
    $md.Add('')
    if ($script:Rdp) {
        $md.Add('> **WARNING: this run was executed in a REMOTE DESKTOP session.**')
        $md.Add('> GPU numbers collected here are meaningless (no physical scanout).')
        $md.Add('')
    }
    if (-not $script:GpuAvailable) {
        $md.Add('> **GPU stage = N/A: this kit build has no GPU backend on Windows.**')
        $md.Add('')
    }
    $md.Add(('- run: {0}  (quick={1}, soak={2} min)' -f $StartTime.ToString('yyyy-MM-dd HH:mm:ss'), [bool]$Quick, $SoakMinutes))
    $md.Add(('- elapsed: {0:N1} min' -f $elapsed))
    $md.Add('')
    $md.Add('## environment')
    $md.Add('')
    $md.Add(('- OS: {0}' -f $script:Env.os))
    $md.Add(('- CPU: {0}' -f $script:Env.cpu))
    $md.Add(('- RAM: {0} GB' -f $script:Env.ram_gb))
    $md.Add(('- session: {0} (RDP={1})' -f $script:Env.sessionname, $script:Env.rdp))
    foreach ($v in @($script:Env.video)) {
        $md.Add(('- GPU: {0} | driver {1} | VRAM {2} MB | {3}' -f $v.name, $v.driver_version, $v.adapter_ram_mb, $v.video_processor))
        $md.Add(('  - pnp: {0}' -f $v.pnp_device_id))
    }
    foreach ($d in @($script:Env.displays)) {
        $md.Add(('- display: {0} dpi {1} scale {2}' -f $d.monitor, $d.dpiX, $d.scale))
    }
    $md.Add(('- Vulkan ICD entries: {0}' -f $script:Env.vulkan_icd_count))
    $md.Add(('- app.exe sha256: {0} ({1} bytes)' -f $script:Env.app_sha256, $script:Env.app_bytes))
    $md.Add('')
    $md.Add('## stages')
    $md.Add('')
    $md.Add('| stage | title | status | seconds | note |')
    $md.Add('|---|---|---|---|---|')
    foreach ($r in $script:Results) {
        $md.Add(('| {0} | {1} | **{2}** | {3} | {4} |' -f $r.stage, $r.title, $r.status, $r.seconds, ($r.error -replace '\|', '/')))
    }
    $md.Add('')
    $md.Add('## numbers')
    $md.Add('')
    foreach ($k in $script:FirstFrame.Keys) {
        $v = $script:FirstFrame[$k]
        if ($v.Count -gt 0) {
            $md.Add(('- first_frame_ms [{0}]: n={1} median={2:N0} min={3:N0}' -f $k, $v.Count, ($v | Sort-Object)[[int]($v.Count / 2)], ($v | Measure-Object -Minimum).Minimum))
        }
    }
    if ($script:Env.ContainsKey('idle_cpu') -and $script:Env.idle_cpu) {
        $last = $script:Env.idle_cpu[-1]
        $md.Add(('- idle memory [cpu] after {0}s: ws {1} MB / priv {2} MB / handles {3} / threads {4}' -f $last.t_s, $last.ws_mb, $last.priv_mb, $last.handles, $last.threads))
    }
    foreach ($tp in 'tp_120x30', 'tp_200x60') {
        if ($script:Env.ContainsKey($tp) -and $script:Env[$tp]) {
            $t = $script:Env[$tp]
            $f = $t.frames
            if ($f) {
                $md.Add(('- throughput {0}: {1}s; render med {2} ms; present med {3} ms; feed {4} B/s; coalesced {5}' -f $tp, $t.seconds, $f['render_med_ms'], $f['present_med_ms'], $f['feed_bytes_per_s'], $f['coalesced']))
            } else {
                $md.Add(('- throughput {0}: {1}s (no frames line)' -f $tp, $t.seconds))
            }
        }
    }
    foreach ($k in 'resize_cmp', 'minimize_cmp', 'cover_cmp') {
        if (-not $script:Env.ContainsKey($k)) { continue }
        $c = $script:Env[$k]
        if ($c) {
            $md.Add(('- {0}: max_diff={1} diff_ratio={2} gt32_ratio={3} blank_rows={4} blank_cols={5}' -f $k, $c.max_diff, $c.diff_ratio, $c.gt32_ratio, $c.blank_rows, $c.blank_cols))
        }
    }
    foreach ($b in 'cpu', 'gpu') {
        $key = 'soak_' + $b
        if ($script:Env.ContainsKey($key) -and $script:Env[$key] -and $script:Env[$key].Count -gt 0) {
            $sk = $script:Env[$key]
            $exitKey = 'soak_exit_' + $b
            $exits = if ($script:Env.ContainsKey($exitKey)) { $script:Env[$exitKey] } else { 'unknown' }
            $md.Add(('- soak [{0}]: {1} samples, ws {2} -> {3} MB, exits={4}' -f $b, $sk.Count, $sk[0].ws_mb, $sk[-1].ws_mb, $exits))
        }
    }
    $md.Add('')
    if ($script:Notes.Count -gt 0) {
        $md.Add('## notes')
        $md.Add('')
        foreach ($n in $script:Notes) { $md.Add('- ' + $n) }
        $md.Add('')
    }
    $md.Add('## artefacts')
    $md.Add('')
    $md.Add('- shots/: S_cpu.png and the post-action screenshots')
    $md.Add('- logs/: app stderr logs (APP_PERF_FRAMES=1, direct file handle)')
    $md.Add('- environment.json, idle-cpu.json, soak-*.json, dxdiag.txt (if dxdiag finished)')
    $md -join "`r`n" | Set-Content -Path (Join-Path $RunDir 'summary.md') -Encoding UTF8

    $json = [pscustomobject]@{
        run = $Stamp
        quick = [bool]$Quick
        soak_minutes = $SoakMinutes
        elapsed_minutes = [math]::Round($elapsed, 2)
        rdp = [bool]$script:Rdp
        gpu_available = [bool]$script:GpuAvailable
        stages = $script:Results
        first_frame = $script:FirstFrame
        notes = $script:Notes
        env = $script:Env
    }
    $json | ConvertTo-Json -Depth 8 | Set-Content -Path (Join-Path $RunDir 'summary.json') -Encoding UTF8

    # always produce the zip, even when stages failed
    $zip = Join-Path $OutDir ('tonight-{0}.zip' -f $Stamp)
    for ($try = 1; $try -le 5; $try++) {
        if (Test-Path $zip) { Remove-Item $zip -Force -ErrorAction SilentlyContinue }
        [GC]::Collect()
        try {
            Compress-Archive -Path (Join-Path $RunDir '*') -DestinationPath $zip -CompressionLevel Optimal -ErrorAction Stop
            break
        } catch {
            if ($try -eq 5) { throw ('zip failed after 5 attempts: ' + $_.Exception.Message) }
            Start-Sleep -Seconds 2
        }
    }
    Write-Host ('ZIP {0} {1} bytes' -f $zip, (Get-Item $zip).Length)
}

# ---------------------------------------------------------------- final report
Write-Host ''
Write-Host ('tonight kit v1 finished: elapsed {0:N1} min' -f ((Get-Date) - $StartTime).TotalMinutes)
foreach ($r in $script:Results) {
    Write-Host ('  {0} {1,-10} {2}' -f $r.stage, $r.status, $(if ($r.error) { $r.error } else { '' }))
}
$finalZip = Get-ChildItem -Path $OutDir -Filter ('tonight-{0}.zip' -f $Stamp) -ErrorAction SilentlyContinue
if ($finalZip) { Write-Host ('results: ' + $finalZip.FullName) } else { Write-Host 'results zip missing' }
