$m = 'C:\dev\tty\vt-verify-win\osc\marker4.txt'
[IO.File]::WriteAllText($m, 'injected-os4')
[Console]::Write([char]27 + ']4;1;rgb:aa/bb/cc' + [char]7)
[Console]::Write([char]27 + '[41m' + (' ' * 10) + [char]27 + '[0m')
[Console]::Write("`r`n")
