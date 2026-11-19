# 8C 第二部分：生成与用户数据集规模相当的目录树（71725 级内的近似：71625 目录 / 116950 文件）。
# 仅用于本地吞吐诊断，不随包发布、不被其它脚本引用（非导出工具）。
param(
    [Parameter(Mandatory = $true)][string]$Root,
    [int]$Tops = 65,
    [int]$Subs = 1100
)
$ErrorActionPreference = 'Stop'
if (Test-Path $Root) { throw "refusing to overwrite existing: $Root" }
[void][IO.Directory]::CreateDirectory($Root)
$dirs = 0; $files = 0
$fileCount = 0
$extraBudget = 45385
for ($t = 1; $t -le $Tops; $t++) {
    $top = Join-Path $Root ("t{0:D3}" -f $t)
    [void][IO.Directory]::CreateDirectory($top); $dirs++
    [IO.File]::WriteAllText((Join-Path $top 'top.txt'), 'x'); $files++
    for ($s = 1; $s -le $Subs; $s++) {
        $sub = Join-Path $top ("s{0:D4}" -f $s)
        [void][IO.Directory]::CreateDirectory($sub); $dirs++
        [IO.File]::WriteAllText((Join-Path $sub 'f0.txt'), 'x'); $files++
        if ($fileCount -lt $extraBudget) {
            [IO.File]::WriteAllText((Join-Path $sub 'f1.txt'), 'x'); $files++
            $fileCount++
        }
    }
}
for ($k = 1; $k -le 60; $k++) {
    [void][IO.Directory]::CreateDirectory((Join-Path $Root ("x{0:D2}" -f $k))); $dirs++
    [IO.File]::WriteAllText((Join-Path $Root ("x{0:D2}" -f $k) 'f.txt'), 'x'); $files++
}
"dirs=$dirs files=$files root=$Root"
