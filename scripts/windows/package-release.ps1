# package-release.ps1 - 5B t7：产出 leantty-0.1.0-windows-x86_64.zip（RC 包）
#
# 全部路径由本脚本位置推导（scripts/windows -> 仓库根）；不写死盘符/用户名。
# zip 用 System.IO.Compression 流式写入（禁用 Compress-Archive：大文件会整包入内存、
# 且不保留顺序/权限）。
#
# 包内容（单一顶层目录 leantty-0.1.0-windows-x86_64/）：
#   leantty.exe / conpty.dll / OpenConsole.exe / leantty.toml / leantty-portable.cmd /
#   LICENSE / THIRD-PARTY-NOTICES.md / README.md / README.zh-CN.md
# 不含：调试符号、日志、开发文档。
param(
    [string]$ConptyDir = '',
    [string]$OutDir = ''
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = (Get-Item $here).Parent.Parent.FullName
$versionLine = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1)
if (-not $versionLine) { throw 'cannot read version from Cargo.toml' }
$version = $versionLine.Matches[0].Groups[1].Value
$top = "leantty-$version-windows-x86_64"

if (-not $OutDir) { $OutDir = Join-Path $repo 'vt-verify-win\dist-release' }
if (-not $ConptyDir) { $ConptyDir = Join-Path $repo 'vt-verify-win\tonight-kit-v4' }
$zipPath = Join-Path (Split-Path -Parent $OutDir) ("{0}.zip" -f $top)

# ConPTY 固定版本（与 3K/3O 记录一致）
$ExpectedConpty = @{
    'conpty.dll'      = 'e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492'
    'OpenConsole.exe' = 'b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775'
}
function Get-Sha256([string]$p) { (Get-FileHash $p -Algorithm SHA256).Hash.ToLowerInvariant() }

$stage = Join-Path ([IO.Path]::GetTempPath()) ("leantty-pkg-" + [Guid]::NewGuid().ToString('N'))
$stageTop = Join-Path $stage $top
New-Item -ItemType Directory -Force -Path $stageTop | Out-Null

try {
    # 1) exe
    $exe = Join-Path $repo 'target\release\leantty.exe'
    if (-not (Test-Path $exe)) { throw "release binary missing: $exe (run build.ps1 first)" }
    Copy-Item $exe (Join-Path $stageTop 'leantty.exe')

    # 2) ConPTY 对：sha256 + Authenticode（Microsoft Corporation）
    foreach ($name in $ExpectedConpty.Keys) {
        $src = Join-Path $ConptyDir $name
        if (-not (Test-Path $src)) { throw "ConPTY file missing: $src" }
        $got = Get-Sha256 $src
        if ($got -ne $ExpectedConpty[$name]) { throw "$name sha256 mismatch: $got" }
        $sig = Get-AuthenticodeSignature $src
        if ($sig.Status -ne 'Valid') { throw "$name signature not valid: $($sig.Status)" }
        if ($sig.SignerCertificate.Subject -notlike '*Microsoft Corporation*') {
            throw "$name signer is not Microsoft Corporation: $($sig.SignerCertificate.Subject)"
        }
        Copy-Item $src (Join-Path $stageTop $name)
    }

    # 3) 配置模板 / 便携启动器 / 授权 / 文档
    Copy-Item (Join-Path $repo 'config\leantty.windows.toml') (Join-Path $stageTop 'leantty.toml')
    Copy-Item (Join-Path $repo 'packaging\windows\leantty-portable.cmd') (Join-Path $stageTop 'leantty-portable.cmd')
    foreach ($f in @('LICENSE', 'THIRD-PARTY-NOTICES.md', 'README.md', 'README.zh-CN.md', 'CHANGELOG.md')) {
        $src = Join-Path $repo "packaging\windows\$f"
        if (-not (Test-Path $src)) { $src = Join-Path $repo $f }
        if (-not (Test-Path $src)) { throw "release doc missing: $f" }
        Copy-Item $src (Join-Path $stageTop $f)
    }

    # 4) 发布 dist 目录（覆盖式）
    if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutDir) | Out-Null
    Copy-Item -Recurse -Force $stageTop $OutDir

    # 5) 流式 zip：单一顶层目录 + 条目按文件名排序
    if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $fs = [IO.File]::Open($zipPath, [IO.FileMode]::CreateNew)
    try {
        $zip = New-Object IO.Compression.ZipArchive($fs, [IO.Compression.ZipArchiveMode]::Create)
        try {
            $files = Get-ChildItem -Path $stageTop -File | Sort-Object Name
            foreach ($f in $files) {
                $entryName = "$top/$($f.Name)"
                $entry = $zip.CreateEntry($entryName, [IO.Compression.CompressionLevel]::Optimal)
                $es = $entry.Open()
                $srcStream = [IO.File]::OpenRead($f.FullName)
                try { $srcStream.CopyTo($es) } finally { $srcStream.Dispose(); $es.Dispose() }
            }
        } finally { $zip.Dispose() }
    } finally { $fs.Dispose() }

    $size = (Get-Item $zipPath).Length
    $sha = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host "zip: $zipPath"
    Write-Host "size: $size bytes"
    Write-Host "sha256: $sha"
    Write-Host "entries:"
    Get-ChildItem $stageTop -File | Sort-Object Name | ForEach-Object {
        Write-Host ("  {0}  {1}  {2}" -f $_.Name, $_.Length, (Get-Sha256 $_.FullName))
    }
} finally {
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
