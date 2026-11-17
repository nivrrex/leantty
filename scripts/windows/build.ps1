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
# 5B t6：路径泄漏治理 —— 把仓库根、CARGO_HOME、RUSTUP_HOME 映射到中性前缀。
# 不能写进 .cargo/config.toml（路径必须运行时取值），所以经 RUSTFLAGS 注入；
# 注意 RUSTFLAGS 会覆盖 config 里 [target.*] 的 rustflags，故这里把
# --no-gc-sections 一并带上（与 config 保持一致）。
$repoRoot = (Get-Item $PSCommandPath).Directory.Parent.Parent.FullName
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $HOME '.rustup' }
$env:RUSTFLAGS = ("-C link-arg=-Wl,--no-gc-sections " +
    "--remap-path-prefix={0}=/leantty " -f $repoRoot) +
    ("--remap-path-prefix={0}=/cargo-home " -f $cargoHome) +
    ("--remap-path-prefix={0}=/rustup-home" -f $rustupHome)
Write-Host "RUSTFLAGS remap: repo=$repoRoot cargo=$cargoHome rustup=$rustupHome"
if ($ExtraCargoArgs) {
    & cargo build --workspace $ExtraCargoArgs
} else {
    & cargo build --workspace
}
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build --workspace failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}
# 5B t5：图标资源由独立脚本处理（windres -> COFF -> cargo rustc 只注入最终 bin）。
# 路径全部由脚本位置推导；不写进 .cargo/config.toml 的 rustflags（会波及 build script 链接）。
$iconScript = Join-Path $PSScriptRoot 'gen-icon-res.ps1'
if (Test-Path $iconScript) {
    & $iconScript -Zig $Zig
    if ($LASTEXITCODE -ne 0) { throw "gen-icon-res.ps1 failed (exit $LASTEXITCODE)" }
}
Write-Host "build.ps1 OK"
