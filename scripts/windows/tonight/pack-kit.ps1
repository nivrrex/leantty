# pack-kit.ps1 - assemble "tonight kit v1" (no GPU build) and zip it.
#
# Contents (all relative to -OutDir):
#   app.exe, conpty.dll, OpenConsole.exe   (verified, see fetch-conpty.ps1 pins)
#   LICENSE-ConPTY.txt, NOTICE.md          (verbatim upstream, sha256 pinned)
#   run-tonight.ps1, lib.ps1, configs\     (the test harness)
#   README-TONIGHT.md, MANIFEST.txt        (one page + per-file sha256)
#
# Every path is a parameter or derived from the repo root; nothing is hard-coded
# to this machine. Any verification failure exits non-zero and removes the partial
# output directory and zip. No binaries ever land inside the repository.
#
# Usage:
#   powershell -File scripts/windows/tonight/pack-kit.ps1
#   powershell -File scripts/windows/tonight/pack-kit.ps1 -OutDir D:\kit\tty -Proxy http://127.0.0.1:7777
param(
    [string]$OutDir = '',
    [string]$TargetDir = '',
    [string]$ConptyDir = '',
    [string]$NoticePath = '',
    [string]$Proxy = '',
    [string]$ZipName = 'tonight-kit-v1'
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
# here = scripts/windows/tonight -> three levels up is the repo root
$repo = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $here))
if (-not $OutDir) { $OutDir = Join-Path $repo 'vt-verify-win\tonight-kit' }
if (-not $TargetDir) { $TargetDir = Join-Path $repo 'target\release' }

# same pins as scripts/windows/package.ps1 (measured in PHASE3K-WIN.md t1)
$ExpectedConpty = @{
    'conpty.dll'      = 'e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492'
    'OpenConsole.exe' = 'b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775'
}
$NoticeCommit = 'f6de9e0618015def1c9dda5b82f9d3abf03eb799'
$NoticeUrl = "https://api.github.com/repos/microsoft/terminal/contents/NOTICE.md?ref=$NoticeCommit"
$ExpectedNoticeSha256 = 'e7fbaadee6ab20c28b87730a510ee5f5815d8fb4bd88d1d54d282dc2a74c0726'

function Get-Sha256([string]$path) { (Get-FileHash -Path $path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Assert-Sha256([string]$path, [string]$expected, [string]$what) {
    if (-not (Test-Path $path)) { throw "$what missing: $path" }
    $a = Get-Sha256 $path
    if ($a -ne $expected) { throw "$what sha256 mismatch: expected $expected, got $a" }
}

$appExe = Join-Path $TargetDir 'app.exe'
$conptyLicense = Join-Path $repo 'scripts\windows\conpty\LICENSE-ConPTY.txt'
foreach ($p in @($appExe, $conptyLicense, (Join-Path $here 'run-tonight.ps1'), (Join-Path $here 'lib.ps1'), (Join-Path $here 'README-TONIGHT.md'), (Join-Path $here 'configs'))) {
    if (-not (Test-Path $p)) { throw "missing input: $p" }
}

$zipPath = Join-Path (Split-Path -Parent $OutDir) ("{0}.zip" -f $ZipName)
$zipExisted = Test-Path $zipPath
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('kit-' + [Guid]::NewGuid().ToString('N'))
$stage = Join-Path $tmp 'stage'
New-Item -ItemType Directory -Force -Path $stage | Out-Null

try {
    Copy-Item $appExe (Join-Path $stage 'app.exe')

    Write-Host '== ConPTY =='
    if ($ConptyDir) {
        foreach ($n in $ExpectedConpty.Keys) {
            $src = Join-Path $ConptyDir $n
            Assert-Sha256 $src $ExpectedConpty[$n] $n
            Copy-Item $src (Join-Path $stage $n)
        }
        Write-Host "reused verified ConPTY files from $ConptyDir"
    } else {
        $fetch = Join-Path $repo 'scripts\windows\conpty\fetch-conpty.ps1'
        $args = @{ OutDir = (Join-Path $tmp 'conpty') }
        if ($Proxy) { $args['Proxy'] = $Proxy }
        & $fetch @args
        if ($LASTEXITCODE -ne 0) { throw "fetch-conpty.ps1 failed with exit code $LASTEXITCODE" }
        foreach ($n in $ExpectedConpty.Keys) {
            $src = Join-Path (Join-Path $tmp 'conpty') $n
            Assert-Sha256 $src $ExpectedConpty[$n] $n
            Copy-Item $src (Join-Path $stage $n)
        }
    }

    Write-Host '== NOTICE.md =='
    $noticeDst = Join-Path $stage 'NOTICE.md'
    if ($NoticePath) {
        Assert-Sha256 $NoticePath $ExpectedNoticeSha256 'NOTICE.md'
        Copy-Item $NoticePath $noticeDst
    } else {
        $nt = Join-Path $tmp 'NOTICE.md'
        $curl = @('-sS', '-L', '--max-time', '120', '-H', 'Accept: application/vnd.github.raw', $NoticeUrl, '-o', $nt)
        if ($Proxy) { $curl = @('-x', $Proxy) + $curl }
        & curl.exe @curl
        if ($LASTEXITCODE -ne 0) { throw "downloading NOTICE.md failed (exit $LASTEXITCODE)" }
        Assert-Sha256 $nt $ExpectedNoticeSha256 'NOTICE.md'
        Copy-Item $nt $noticeDst
    }
    Copy-Item $conptyLicense (Join-Path $stage 'LICENSE-ConPTY.txt')

    # harness + docs
    Copy-Item (Join-Path $here 'run-tonight.ps1') $stage
    Copy-Item (Join-Path $here 'lib.ps1') $stage
    Copy-Item (Join-Path $here 'README-TONIGHT.md') $stage
    $cfgOut = Join-Path $stage 'configs'
    New-Item -ItemType Directory -Force -Path $cfgOut | Out-Null
    Get-ChildItem (Join-Path $here 'configs') -Filter *.toml | ForEach-Object { Copy-Item $_.FullName $cfgOut }

    # per-file manifest (relative paths, sha256, size)
    $lines = @('# relative_path  bytes  sha256')
    foreach ($f in (Get-ChildItem -Path $stage -Recurse -File | Sort-Object FullName)) {
        $rel = $f.FullName.Substring($stage.Length + 1).Replace('\', '/')
        $lines += ('{0}  {1}  {2}' -f $rel, $f.Length, (Get-Sha256 $f.FullName))
    }
    $lines -join "`r`n" | Set-Content -Path (Join-Path $stage 'MANIFEST.txt') -Encoding ASCII

    if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutDir) | Out-Null
    Copy-Item -Recurse -Force $stage $OutDir
    if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
    Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $zipPath -CompressionLevel Optimal

    $dirBytes = (Get-ChildItem -Path $OutDir -Recurse -File | Measure-Object -Property Length -Sum).Sum
    Write-Host ('KIT dir={0} files={1} bytes={2}' -f $OutDir, (Get-ChildItem -Path $OutDir -Recurse -File).Count, $dirBytes)
    Write-Host ('ZIP path={0} bytes={1} sha256={2}' -f $zipPath, (Get-Item $zipPath).Length, (Get-Sha256 $zipPath))
    Write-Host 'MANIFEST:'
    Get-Content (Join-Path $OutDir 'MANIFEST.txt') | ForEach-Object { Write-Host ('  ' + $_) }
    Write-Host 'pack-kit.ps1 OK'
} catch {
    if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir -ErrorAction SilentlyContinue }
    if ((-not $zipExisted) -and (Test-Path $zipPath)) { Remove-Item -Force $zipPath -ErrorAction SilentlyContinue }
    Write-Error ('pack-kit.ps1 failed: ' + $_.Exception.Message)
    exit 1
} finally {
    if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
}
