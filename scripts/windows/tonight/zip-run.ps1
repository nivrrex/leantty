# zip-run.ps1 - streams a results directory into a zip archive.
# Runs as its own process on purpose: the caller has already loaded WinForms /
# System.Drawing, and mixing those with System.IO.Compression in one session was
# observed to stall on this machine.
# Rules: files > MaxFileMB contribute only their last 5 MB (reported), per-file
# failures are skipped (reported), the output zip is written to a .partial file
# first and renamed at the end.
# ASCII only.
param(
    [Parameter(Mandatory = $true)][string]$RunDir,
    [Parameter(Mandatory = $true)][string]$OutZip,
    [int]$MaxFileMB = 50,
    [switch]$SkipBig
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$limit = [int64]$MaxFileMB * 1MB
$tail = [int64]5 * 1MB
$partial = $OutZip + '.partial'
if (Test-Path $partial) { Remove-Item $partial -Force -ErrorAction SilentlyContinue }
$truncated = @()
$skipped = @()
$count = 0
$bytes = [int64]0
$fs = [IO.File]::Open($partial, 'Create', 'Write')
try {
    $arch = New-Object System.IO.Compression.ZipArchive($fs, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($f in (Get-ChildItem -Path $RunDir -Recurse -File | Sort-Object FullName)) {
            if ($f.FullName -eq $OutZip -or $f.FullName -eq $partial) { continue }
            $rel = $f.FullName.Substring($RunDir.Length + 1).Replace([char]92, '/')
            if ($SkipBig -and $f.Length -gt $limit) { $skipped += $rel; continue }
            $seekFrom = [int64]0
            if ($f.Length -gt $limit) { $seekFrom = $f.Length - $tail; $truncated += $rel }
            try {
                $e = $arch.CreateEntry($rel, [System.IO.Compression.CompressionLevel]::Fastest)
                $es = $e.Open()
                try {
                    $src = [IO.File]::Open($f.FullName, 'Open', 'Read', 'ReadWrite')
                    try {
                        if ($seekFrom -gt 0) { [void]$src.Seek($seekFrom, 'Begin') }
                        $buf = New-Object byte[] 65536
                        while (($n = $src.Read($buf, 0, $buf.Length)) -gt 0) { $es.Write($buf, 0, $n); $bytes += $n }
                    } finally { $src.Dispose() }
                } finally { $es.Dispose() }
                $count++
            } catch {
                $skipped += ($rel + ' (' + $_.Exception.Message + ')')
            }
        }
    } finally { $arch.Dispose() }
} finally { $fs.Dispose() }
if (Test-Path $OutZip) { Remove-Item $OutZip -Force -ErrorAction SilentlyContinue }
Move-Item $partial $OutZip -Force
$o = [pscustomobject]@{
    ok = $true
    zip = $OutZip
    zip_bytes = (Get-Item $OutZip).Length
    entries = $count
    payload_bytes = $bytes
    truncated = $truncated
    skipped = $skipped
}
$o | ConvertTo-Json -Depth 4 -Compress
