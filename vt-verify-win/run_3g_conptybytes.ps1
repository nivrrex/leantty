# t4: capture the bytes ConPTY delivers for the sequences that matter, using the
# raw PTY reader probe (probe_ptyin), i.e. without going through the app.
# Run once per (condition, sequence, generator) and store the raw hex.
param(
    [string]$Tag = 'system',
    [string]$Ptyin = 'C:\dev\tty\vt-verify-win\probe_ptyin\target\release\ptyin.exe',
    [string]$Pwsh = 'C:\Program Files\PowerShell\7\pwsh.exe',
    [string]$Bash = 'C:\Program Files\Git\bin\bash.exe'
)
$ROOT = 'C:\dev\tty'
$out = Join-Path $ROOT 'vt-verify-win\logs\conpty-seq'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$seqs = @('s1_inverse_default', 's2_inverse_white', 's3_red_text', 's4_osc4_red', 's5_osc0_title')
if (-not (Test-Path $Bash)) { $Bash = (Get-Command bash.exe -ErrorAction SilentlyContinue).Source }
foreach ($s in $seqs) {
    $ps1 = Join-Path $ROOT ("vt-verify-win\conpty-seq\$s.ps1")
    # generator A: pwsh -File
    $logA = Join-Path $out ("{0}-{1}-pwsh.txt" -f $Tag, $s)
    & $Ptyin $Pwsh -NoProfile -File $ps1 *> $logA
    Write-Host ("T4 {0} {1} pwsh rc={2}" -f $Tag, $s, $LASTEXITCODE)
    # generator B: bash printf (a native emitter that writes raw bytes)
    if ($Bash) {
        $logB = Join-Path $out ("{0}-{1}-bash.txt" -f $Tag, $s)
        $map = @{
            's1_inverse_default' = "printf '\033[7m          \033[0m'"
            's2_inverse_white'   = "printf '\033[37m\033[7m          \033[0m'"
            's3_red_text'        = "printf '\033[31mREDTEXT\033[0m'"
            's4_osc4_red'        = "printf '\033]4;1;rgb:aa/bb/cc\007\033[31mOSCRED\033[0m\033]104\007'"
            's5_osc0_title'      = "printf '\033]0;T3GTITLE\007'"
        }
        & $Ptyin $Bash -lc $map[$s] *> $logB
        Write-Host ("T4 {0} {1} bash rc={2}" -f $Tag, $s, $LASTEXITCODE)
    }
}
Write-Host ("T4 {0} done" -f $Tag)
