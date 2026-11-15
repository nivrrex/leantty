param([string]$Tag = 'probe')
$ROOT = 'C:\dev\tty'
$out = Join-Path $ROOT 'vt-verify-win\logs'
function Measure-File([string]$path, [string]$label, [int]$iters = 10) {
    if (-not (Test-Path $path)) { return "$label missing" }
    $firstByte = @(); $full = @(); $hot = @()
    for ($i = 0; $i -lt $iters; $i++) {
        $t0 = [Diagnostics.Stopwatch]::GetTimestamp()
        $fs = [IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
        $b = $fs.ReadByte()
        $t1 = [Diagnostics.Stopwatch]::GetTimestamp()
        $firstByte += (($t1 - $t0) * 1000000 / [Diagnostics.Stopwatch]::Frequency) / 1000.0
        $t2 = [Diagnostics.Stopwatch]::GetTimestamp()
        $buf = New-Object byte[] (1MB); $total = 0
        $fs.Position = 0
        while (($n = $fs.Read($buf, 0, $buf.Length)) -gt 0) { $total += $n }
        $t3 = [Diagnostics.Stopwatch]::GetTimestamp()
        $full += (($t3 - $t2) * 1000000 / [Diagnostics.Stopwatch]::Frequency) / 1000.0
        $fs.Dispose()
        $t4 = [Diagnostics.Stopwatch]::GetTimestamp()
        $fs2 = [IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
        [void]$fs2.ReadByte()
        $t5 = [Diagnostics.Stopwatch]::GetTimestamp()
        $hot += (($t5 - $t4) * 1000000 / [Diagnostics.Stopwatch]::Frequency) / 1000.0
        $fs2.Dispose()
    }
    $med = { param($a) ($a | Sort-Object)[[int]($a.Count / 2)] }
    "{0}: firstByte med={1:N2}ms min={2:N2} | full med={3:N2}ms min={4:N2} | hotOpen med={5:N2}ms min={6:N2}" -f `
        $label, (& $med $firstByte), ($firstByte | Measure-Object -Minimum).Minimum, `
        (& $med $full), ($full | Measure-Object -Minimum).Minimum, `
        (& $med $hot), ($hot | Measure-Object -Minimum).Minimum
}
$tmp = Join-Path ([IO.Path]::GetTempPath()) 'conpty-font-probe'
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$res = @()
foreach ($f in 'consola.ttf', 'msyh.ttc') {
    $src = Join-Path 'C:\Windows\Fonts' $f
    $res += (Measure-File $src ("{0} (system path)" -f $f))
    if (Test-Path $src) {
        $dst = Join-Path $tmp $f
        Copy-Item $src $dst -Force
        $res += (Measure-File $dst ("{0} (temp copy)" -f $f))
    }
}
$res | ForEach-Object { Write-Host ("T5 " + $_) }
