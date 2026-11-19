# fetch-conpty.ps1 - download and verify the official Windows ConPTY redistributable
#
# The script is parameterised; nothing about the local machine is hard-coded.
# Proxy: pass -Proxy (or set the HTTPS_PROXY env var) when raw.githubusercontent is
# not reachable; the proxy value is never written into the repository.
#
# Verification chain (any failure aborts with a non-zero exit code and leaves no
# partial output directory behind):
#   1. package SHA-256 matches the pinned value below
#   2. the two extracted files match their pinned SHA-256 values
#   3. both files carry a valid Authenticode signature from Microsoft Corporation
#
# Usage:
#   pwsh -File scripts/windows/conpty/fetch-conpty.ps1 -OutDir <path>
#   pwsh -File scripts/windows/conpty/fetch-conpty.ps1 -OutDir <path> -Proxy <proxy-url>
param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Proxy = '',
    [string]$PackageId = 'Microsoft.Windows.Console.ConPTY',
    [string]$Version = '1.25.261109001'
)
$ErrorActionPreference = 'Stop'

# --- pinned expectations (measured on 2026-11-15, see PHASE3K-WIN.md t1) ------
$ExpectedPackageSha256 = '3d77f535df848e44225c2a8912ac844fecb23f3057732b639f3034b587496f09'
$ExpectedFiles = @{
    'conpty.dll'      = 'e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492'
    'OpenConsole.exe' = 'b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775'
}
$SignerSubject = 'CN=Microsoft Corporation'

function New-ProxyArgs([string]$p) {
    # named-parameter splatting needs a hashtable (an array would be positional)
    $h = @{}
    if (-not [string]::IsNullOrWhiteSpace($p)) { $h['Proxy'] = $p }
    return $h
}
function Get-Sha256([string]$path) {
    (Get-FileHash -Path $path -Algorithm SHA256).Hash.ToLowerInvariant()
}

$base = "https://api.nuget.org/v3-flatcontainer/$($PackageId.ToLowerInvariant())/$Version/$($PackageId.ToLowerInvariant()).$Version.nupkg"
$stamp = [Guid]::NewGuid().ToString('N')
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "conpty-$stamp"
$stage = Join-Path ([System.IO.Path]::GetTempPath()) "conpty-out-$stamp"
New-Item -ItemType Directory -Force -Path $tmp, $stage | Out-Null
try {
    $nupkg = Join-Path $tmp 'pack.nupkg'
    $proxyArgs = New-ProxyArgs $Proxy
    if ($proxyArgs.ContainsKey('Proxy')) { Write-Host "using proxy $Proxy" }
    Invoke-WebRequest -Uri $base -OutFile $nupkg -UseBasicParsing @proxyArgs

    $actual = Get-Sha256 $nupkg
    Write-Host "package sha256 = $actual"
    if ($actual -ne $ExpectedPackageSha256) {
        throw "package hash mismatch: expected $ExpectedPackageSha256, got $actual"
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($nupkg)
    try {
        $map = @{
            'conpty.dll'      = 'runtimes/win-x64/native/conpty.dll'
            'OpenConsole.exe' = 'build/native/runtimes/x64/OpenConsole.exe'
        }
        foreach ($name in $map.Keys) {
            $entry = $zip.Entries | Where-Object { $_.FullName -eq $map[$name] }
            if (-not $entry) { throw "package does not contain $($map[$name])" }
            $dst = Join-Path $stage $name
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dst, $true)
            $h = Get-Sha256 $dst
            Write-Host "$name sha256 = $h"
            if ($h -ne $ExpectedFiles[$name]) {
                throw "$name hash mismatch: expected $($ExpectedFiles[$name]), got $h"
            }
            $sig = Get-AuthenticodeSignature -FilePath $dst
            if ($sig.Status -ne 'Valid') { throw "$name signature status = $($sig.Status)" }
            $subject = $sig.SignerCertificate.Subject
            Write-Host "$name signer = $subject"
            if ($subject -notlike "$SignerSubject*") { throw "$name signer is not Microsoft: $subject" }
        }
    } finally {
        $zip.Dispose()
    }

    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    foreach ($name in $ExpectedFiles.Keys) {
        Copy-Item -Path (Join-Path $stage $name) -Destination (Join-Path $OutDir $name) -Force
    }
    Write-Host "OK: conpty.dll + OpenConsole.exe verified and copied to $OutDir"
} catch {
    # never leave a half-written output directory behind
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue }
    Write-Error "fetch-conpty failed: $($_.Exception.Message)"
    exit 1
} finally {
    if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue }
}
