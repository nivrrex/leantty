for ($i = 0; $i -lt 80; $i++) {
    [Console]::Write([char]27 + "]0;T123" + [char]7)
    Start-Sleep -Milliseconds 250
}
