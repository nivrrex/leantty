# Windows build script: set the env-var contract and run cargo build --workspace.
# All paths come in as parameters; no absolute path is hard-coded in the repo.
param(
    [Parameter(Mandatory = $true)][string]$Zig,
    [Parameter(Mandatory = $true)][string]$GhosttySourceDir,
    [Parameter(Mandatory = $true)][string]$ZigSystemDir,
    [string]$ExtraCargoArgs
)
$ErrorActionPreference = 'Stop'

foreach ($p in @($Zig, $GhosttySourceDir, $ZigSystemDir)) {
    if (-not (Test-Path $p)) {
        Write-Error "path not found: $p"
        Exit 1
    }
}
if (-not (Test-Path (Join-Path $GhosttySourceDir 'build.zig'))) {
    Write-Error "GHOSTTY_SOURCE_DIR does not contain build.zig: $GhosttySourceDir"
    Exit 1
}

$repo = Split-Path -Parent $PSScriptRoot
$wrapper = Join-Path $PSScriptRoot 'zig-inject-target.cmd'

$env:ZIG_REAL = $Zig
$env:ZIG = $wrapper
$env:GHOSTTY_SOURCE_DIR = $GhosttySourceDir
$env:GHOSTTY_ZIG_SYSTEM_DIR = $ZigSystemDir

Write-Host "ZIG=$env:ZIG (wrapper over ZIG_REAL=$env:ZIG_REAL)"
Write-Host "GHOSTTY_SOURCE_DIR=$env:GHOSTTY_SOURCE_DIR"
Write-Host "GHOSTTY_ZIG_SYSTEM_DIR=$env:GHOSTTY_ZIG_SYSTEM_DIR"

Set-Location $repo
if ($ExtraCargoArgs) {
    & cargo build --workspace $ExtraCargoArgs
} else {
    & cargo build --workspace
}
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build --workspace failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}
Write-Host "build.ps1 OK"
