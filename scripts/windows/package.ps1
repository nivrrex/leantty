# package.ps1 - assemble a redistributable Windows dist directory and a zip archive.
#
# Nothing machine specific is hard-coded: every path is a parameter or derived from
# the repository root (the parent of scripts/windows). The default output directory
# lives under vt-verify-win/ which is git-ignored, so binaries never enter the repo.
#
# Contents of the dist directory:
#   app.exe                 release build of the terminal
#   conpty.dll              official Windows ConPTY (verified, see fetch-conpty.ps1)
#   OpenConsole.exe         ConPTY host (same package, same directory)
#   config.example.toml     copy of examples/windows.toml
#   LICENSE-ConPTY.txt      MIT licence text of the ConPTY package
#   NOTICE.md               upstream microsoft/terminal NOTICE.md, verbatim
#   README.md               one page: run, roll back to system conhost, upgrade ConPTY
#   MANIFEST.txt            sha256 + size of every file above
#
# Any failed verification aborts with a non-zero exit code and removes the partial
# output directory and zip.
#
# Usage:
#   pwsh -File scripts/windows/package.ps1
#   pwsh -File scripts/windows/package.ps1 -OutDir D:\dist\tty -Proxy http://127.0.0.1:7777
param(
    [string]$OutDir = '',
    [string]$TargetDir = '',
    [string]$ConptyDir = '',
    [string]$Proxy = '',
    [string]$NoticePath = '',
    [string]$ZipName = 'tty-win-x64'
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutDir) { $OutDir = Join-Path $repo 'vt-verify-win\dist' }
if (-not $TargetDir) { $TargetDir = Join-Path $repo 'target\release' }
$scriptDir = $PSScriptRoot

# --- pinned expectations -----------------------------------------------------
# ConPTY files: same pins as scripts/windows/conpty/fetch-conpty.ps1 (measured in
# PHASE3K-WIN.md t1). NOTICE.md: content pinned to an upstream commit so the script
# fails loudly if upstream rewrites history or the download is intercepted.
$ExpectedConpty = @{
    'conpty.dll'      = 'e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492'
    'OpenConsole.exe' = 'b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775'
}
$NoticeCommit = 'f6de9e0618015def1c9dda5b82f9d3abf03eb799'
$NoticeUrl = "https://api.github.com/repos/microsoft/terminal/contents/NOTICE.md?ref=$NoticeCommit"
$ExpectedNoticeSha256 = 'e7fbaadee6ab20c28b87730a510ee5f5815d8fb4bd88d1d54d282dc2a74c0726'

function Get-Sha256([string]$path) {
    (Get-FileHash -Path $path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Sha256([string]$path, [string]$expected, [string]$what) {
    if (-not (Test-Path $path)) { throw "$what missing: $path" }
    $actual = Get-Sha256 $path
    if ($actual -ne $expected) { throw "$what sha256 mismatch: expected $expected, got $actual" }
}

# --- pre-flight: every input must exist before anything is written ------------
$appExe = Join-Path $TargetDir 'app.exe'
if (-not (Test-Path $appExe)) { throw "release binary missing: $appExe (build it first)" }
$exampleCfg = Join-Path $repo 'examples\windows.toml'
if (-not (Test-Path $exampleCfg)) { throw "example config missing: $exampleCfg" }
$conptyLicense = Join-Path $scriptDir 'conpty\LICENSE-ConPTY.txt'
if (-not (Test-Path $conptyLicense)) { throw "ConPTY licence text missing: $conptyLicense" }

$zipPath = Join-Path (Split-Path -Parent $OutDir) ("{0}.zip" -f $ZipName)
# remember whether the archive already existed: a failing run must not delete a
# previously good archive that lives next to a different -OutDir
$zipExisted = Test-Path $zipPath
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("tty-dist-" + [Guid]::NewGuid().ToString('N'))
$stage = Join-Path $tmp 'stage'
New-Item -ItemType Directory -Force -Path $stage | Out-Null

try {
    # 1) app binary
    Copy-Item $appExe (Join-Path $stage 'app.exe')

    # 2) ConPTY pair: reuse a verified directory when given, else run the fetcher
    Write-Host "== ConPTY =="
    if ($ConptyDir) {
        foreach ($name in $ExpectedConpty.Keys) {
            $src = Join-Path $ConptyDir $name
            Assert-Sha256 $src $ExpectedConpty[$name] $name
            Copy-Item $src (Join-Path $stage $name)
        }
        Write-Host "reused verified ConPTY files from $ConptyDir"
    } else {
        $fetchSrc = Join-Path $scriptDir 'conpty\fetch-conpty.ps1'
        $fetchArgs = @{ OutDir = (Join-Path $tmp 'conpty') }
        if ($Proxy) { $fetchArgs['Proxy'] = $Proxy }
        & $fetchSrc @fetchArgs
        if ($LASTEXITCODE -ne 0) { throw "fetch-conpty.ps1 failed with exit code $LASTEXITCODE" }
        foreach ($name in $ExpectedConpty.Keys) {
            $src = Join-Path (Join-Path $tmp 'conpty') $name
            Assert-Sha256 $src $ExpectedConpty[$name] $name
            Copy-Item $src (Join-Path $stage $name)
        }
    }

    # 3) config example (renamed so it is obviously a sample, not the live config)
    Copy-Item $exampleCfg (Join-Path $stage 'config.example.toml')

    # 4) ConPTY licence text
    Copy-Item $conptyLicense (Join-Path $stage 'LICENSE-ConPTY.txt')

    # 5) upstream NOTICE.md, verbatim, hash pinned
    Write-Host "== NOTICE.md =="
    $noticeDst = Join-Path $stage 'NOTICE.md'
    if ($NoticePath) {
        Assert-Sha256 $NoticePath $ExpectedNoticeSha256 'NOTICE.md'
        Copy-Item $NoticePath $noticeDst
    } else {
        $noticeTmp = Join-Path $tmp 'NOTICE.md'
        $curlArgs = @('-sS', '-L', '--max-time', '120', '-H', 'Accept: application/vnd.github.raw', $NoticeUrl, '-o', $noticeTmp)
        if ($Proxy) { $curlArgs = @('-x', $Proxy) + $curlArgs }
        & curl.exe @curlArgs
        if ($LASTEXITCODE -ne 0) { throw "downloading NOTICE.md failed (exit $LASTEXITCODE)" }
        Assert-Sha256 $noticeTmp $ExpectedNoticeSha256 'NOTICE.md'
        Copy-Item $noticeTmp $noticeDst
    }

    # 6) README (one page)
    $readme = @'
# tty (Windows x64) - a terminal front-end

## Run

    app.exe --config config.example.toml

`app.exe` and `conpty.dll` / `OpenConsole.exe` must stay in the same directory:
the PTY backend loads conpty.dll from the executable's own directory and uses
the OpenConsole.exe next to it as the console host. Copy the whole directory.

Edit `config.example.toml` (font, colours, key/mouse bindings) and pass it with
`--config`. Without `--config` the built-in defaults are used.

## Roll back to the system conhost

Delete (or rename) both `conpty.dll` and `OpenConsole.exe` in this directory and
restart `app.exe`. The backend falls back to the system ConPTY shipped with
Windows. Visible differences on the system host: wrapped long lines arrive as
CRLF-separated segments, `ESC[7m` (inverse) is rewritten into explicit
foreground/background colours, and `OSC 104` (reset palette) is dropped.

Deleting only `OpenConsole.exe` while keeping `conpty.dll` is NOT a supported
combination; see the report for the measured behaviour.

## Upgrade ConPTY

The two files come from the NuGet package `Microsoft.Windows.Console.ConPTY`
(MIT). To upgrade, re-run the fetcher in a checkout of the source repository:

    pwsh -File scripts/windows/conpty/fetch-conpty.ps1 -OutDir <dir>

It pins the package version and the SHA-256 of the package and of both files and
checks the Authenticode signature (must be Microsoft Corporation), so a new
version needs new pins (see PHASE3K-WIN.md t1 for the values used here).

## Licence

`conpty.dll` and `OpenConsole.exe` are (c) Microsoft Corporation, MIT licensed;
see LICENSE-ConPTY.txt. Third-party notices for those binaries are in
NOTICE.md (copied verbatim from microsoft/terminal). The terminal itself ships
without a licence file in this archive.
'@
    $readme | Set-Content -Path (Join-Path $stage 'README.md') -Encoding UTF8

    # 7) manifest (sha256 + size of every payload file; ConPTY 版本单列一行)
    $conptyVer = (Get-Item (Join-Path $stage 'OpenConsole.exe')).VersionInfo.FileVersion
    $lines = @("# conpty: OpenConsole.exe FileVersion $conptyVer", '# name  bytes  sha256')
    foreach ($f in Get-ChildItem -Path $stage -File | Sort-Object Name) {
        $lines += ('{0}  {1}  {2}' -f $f.Name, $f.Length, (Get-Sha256 $f.FullName))
    }
    $lines -join "`r`n" | Set-Content -Path (Join-Path $stage 'MANIFEST.txt') -Encoding ASCII

    # 8) publish: replace the output directory, then zip
    if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutDir) | Out-Null
    Copy-Item -Recurse -Force $stage $OutDir
    if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
    Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $zipPath -CompressionLevel Optimal

    # 9) report
    $dirBytes = (Get-ChildItem -Path $OutDir -Recurse -File | Measure-Object -Property Length -Sum).Sum
    $zipBytes = (Get-Item $zipPath).Length
    Write-Host ("DIST dir={0} files={1} bytes={2}" -f $OutDir, (Get-ChildItem -Path $OutDir -File).Count, $dirBytes)
    Write-Host ("ZIP path={0} bytes={1} sha256={2}" -f $zipPath, $zipBytes, (Get-Sha256 $zipPath))
    Write-Host "MANIFEST:"
    Get-Content (Join-Path $OutDir 'MANIFEST.txt') | ForEach-Object { Write-Host "  $_" }
    Write-Host "package.ps1 OK"
} catch {
    if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir -ErrorAction SilentlyContinue }
    if ((-not $zipExisted) -and (Test-Path $zipPath)) { Remove-Item -Force $zipPath -ErrorAction SilentlyContinue }
    Write-Error "package.ps1 failed: $($_.Exception.Message)"
    exit 1
} finally {
    if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
}
