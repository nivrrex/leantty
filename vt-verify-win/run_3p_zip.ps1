# t4: reproduce the P4 failure modes and compare zip strategies.
#   A) Compress-Archive (PowerShell 5.1) on a realistic run dir
#   B) the same with one large (60 MB) file
#   C) a log still held open by a writer (leftover app scenario)
#   D) streaming ZipArchive (the new kit implementation)
# Every measurement records wall time and the PowerShell process peak working set.
# ASCII only.
param([string]$Base = 'C:\dev\tty\vt-verify-win\3p-oom')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
if (Test-Path $Base) { Remove-Item -Recurse -Force $Base }
New-Item -ItemType Directory -Force -Path $Base | Out-Null

function New-RunDir([string]$name, [int]$smallFiles = 40, [int]$bigMB = 0) {
    $d = Join-Path $Base $name
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $d 'logs') | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $d 'shots') | Out-Null
    'summary' | Set-Content (Join-Path $d 'summary.md')
    for ($i = 1; $i -le $smallFiles; $i++) {
        ('perf line {0} ' -f $i) * 200 | Set-Content (Join-Path $d ('logs\app-{0}.log' -f $i))
    }
    if ($bigMB -gt 0) {
        $fs = [IO.File]::Open((Join-Path $d 'logs\big.log'), 'Create', 'Write')
        $buf = New-Object byte[] (1MB)
        for ($i = 0; $i -lt $bigMB; $i++) { $fs.Write($buf, 0, $buf.Length) }
        $fs.Dispose()
    }
    return $d
}

function Get-PeakMb {
    $p = Get-Process -Id $PID
    $p.Refresh()
    return [math]::Round($p.PeakWorkingSet64 / 1MB, 1)
}

function Measure-Zip([string]$tag, [string]$dir, [string]$mode) {
    $zip = Join-Path $Base ($tag + '.zip')
    if (Test-Path $zip) { Remove-Item $zip -Force }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $status = 'ok'
    $err = ''
    try {
        if ($mode -eq 'compress') {
            Compress-Archive -Path (Join-Path $dir '*') -DestinationPath $zip -CompressionLevel Optimal -ErrorAction Stop
        } else {
            $fs = [IO.File]::Open($zip, 'Create', 'Write')
            try {
                $arch = New-Object System.IO.Compression.ZipArchive($fs, [System.IO.Compression.ZipArchiveMode]::Create)
                try {
                    foreach ($f in (Get-ChildItem $dir -Recurse -File | Sort-Object FullName)) {
                        $rel = $f.FullName.Substring($dir.Length + 1).Replace('\', '/')
                        $entry = $arch.CreateEntry($rel, [System.IO.Compression.CompressionLevel]::Fastest)
                        $es = $entry.Open()
                        try {
                            $src = [IO.File]::Open($f.FullName, 'Open', 'Read', 'ReadWrite')
                            try {
                                $buf = New-Object byte[] 65536
                                while (($n = $src.Read($buf, 0, $buf.Length)) -gt 0) { $es.Write($buf, 0, $n) }
                            } finally { $src.Dispose() }
                        } finally { $es.Dispose() }
                    }
                } finally { $arch.Dispose() }
            } finally { $fs.Dispose() }
        }
    } catch {
        $status = 'FAILED'
        $err = $_.Exception.Message
    }
    $sw.Stop()
    $bytes = if (Test-Path $zip) { (Get-Item $zip).Length } else { 0 }
    Write-Host ('T4 {0,-14} mode={1,-8} {2,-6} {3,8:N2}s zip={4,10:N0}B peakMb={5}' -f $tag, $mode, $status, $sw.Elapsed.TotalSeconds, $bytes, (Get-PeakMb))
    if ($err) { Write-Host ('   err=' + $err) }
    return [pscustomobject]@{ tag = $tag; mode = $mode; status = $status; seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2); bytes = $bytes; peak_mb = (Get-PeakMb); err = $err }
}

$results = @()
$d1 = New-RunDir 'run-small' 40 0
$results += Measure-Zip 'small-compress' $d1 'compress'
$results += Measure-Zip 'small-stream' $d1 'stream'

$d2 = New-RunDir 'run-big' 40 60
$results += Measure-Zip 'big-compress' $d2 'compress'
$results += Measure-Zip 'big-stream' $d2 'stream'

# C) a log still open for writing (the leftover-app scenario)
$d3 = New-RunDir 'run-locked' 10 0
$locked = Join-Path $d3 'logs\held.log'
$holder = [IO.File]::Open($locked, 'Create', 'Write')
$holder.Write([Text.Encoding]::ASCII.GetBytes('held'), 0, 4)
$holder.Flush()
$results += Measure-Zip 'locked-compress' $d3 'compress'
$results += Measure-Zip 'locked-stream' $d3 'stream'
$holder.Dispose()

$results | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Base 't4-results.json') -Encoding UTF8
Write-Host 'T4-DONE'
