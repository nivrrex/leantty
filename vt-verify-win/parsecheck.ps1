param([string[]]$Files)
foreach ($f in $Files) {
    $t = $null; $e = $null
    [System.Management.Automation.Language.Parser]::ParseFile($f, [ref]$t, [ref]$e) | Out-Null
    if ($e.Count -eq 0) { Write-Host ("OK   " + $f) } else {
        Write-Host ("FAIL " + $f + " (" + $e.Count + ")")
        $e | Select-Object -First 4 | ForEach-Object { Write-Host ("   " + $_.Extent.StartLineNumber + ": " + $_.Message) }
    }
}
