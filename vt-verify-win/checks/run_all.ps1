# W1~W8 one-command runner (Windows native re-verification of Linux V1~V8 + W5 ConPTY special)
# Methodology notes:
#  - W2 uses w2b_route.ps1 (default-route removal = REAL network cut). w2_offline.ps1 is kept
#    only as the documented failed firewall attempt (firewall profiles were Disabled on this
#    host and block rules proved inert; see REPORT-windows.md W2).
#  - W6 exits nonzero BY DESIGN when the Ctrl-C-through-ConPTY byte path does not interrupt
#    ping -t; that failure is the recorded finding, not a harness error.
#  - W3/W4..W8 link via checks/v3_project/.cargo/config.toml (--no-gc-sections + linker wrapper).
$ErrorActionPreference = 'Continue'
$root = 'C:\vt'
$order = @('w1_build', 'w2b_route', 'w3_downstream', 'w4_grid', 'w5_conpty', 'w6_keyencoder', 'w7_scroll', 'w8_release')
$results = @()
foreach ($w in $order) {
    Write-Host "============================================================"
    Write-Host "RUN $w"
    Write-Host "============================================================"
    $log = "$root\logs\$w-script.log"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$root\checks\$w.ps1" *>&1 | Tee-Object -FilePath $log
    $rc = $LASTEXITCODE
    $results += [PSCustomObject]@{ Item = $w; ExitCode = $rc }
    Write-Host "$w exit=$rc"
}
Write-Host "============================================================"
$results | Format-Table -AutoSize
$fail = ($results | Where-Object { $_.ExitCode -ne 0 }).Count
Write-Host "PASS=$($results.Count - $fail) FAIL=$fail (W6 nonzero exit is the recorded Ctrl-C finding)"
exit $fail
