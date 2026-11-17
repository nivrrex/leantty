# 5B t5：把 assets/icon/leantty.ico 编成 COFF 资源对象并链进最终 leantty.exe。
# 所有路径由本脚本位置推导（scripts/windows -> 仓库根），不写死盘符/用户目录。
# 说明：资源对象不能写进 .cargo/config.toml 的 [target.*] rustflags —— 那段也会作用于
# build script / proc-macro 的宿主链接，实测 collect2 链接失败；这里用 cargo rustc 的
# 附加参数，只作用于最终 bin。
param(
    [string]$Zig = '',
    [string]$OutDir = ''
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = (Get-Item $here).Parent.Parent.FullName
$rc = Join-Path $repo 'assets\icon\leantty.rc'
if (-not (Test-Path $rc)) { throw "icon resource script missing: $rc" }
$iconDir = Join-Path $repo 'assets\icon'
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\windows' }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$obj = Join-Path $OutDir 'leantty-res.o'

$windres = Get-Command windres.exe -ErrorAction SilentlyContinue
if ($windres) {
    & $windres.Source -i $rc -I $iconDir -o $obj
    if ($LASTEXITCODE -ne 0) { throw "windres failed (exit $LASTEXITCODE)" }
    Write-Host "icon: windres -> $obj"
} elseif ($Zig) {
    & $Zig rc /fo $obj $rc
    if ($LASTEXITCODE -ne 0) { throw "zig rc failed (exit $LASTEXITCODE)" }
    Write-Host "icon: zig rc -> $obj"
} else {
    throw "icon: neither windres.exe nor -Zig was available"
}

Push-Location $repo
try {
    & cargo rustc --release -p leantty-app --bin leantty -- -C "link-arg=$obj"
    if ($LASTEXITCODE -ne 0) { throw "cargo rustc (icon link) failed (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}
Write-Host "icon linked into leantty.exe"
