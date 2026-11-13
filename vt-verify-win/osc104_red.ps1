Write-Host -NoNewline ([char]27 + "]4;1;rgb:aa/bb/cc" + [char]27 + "\\")
Write-Host -NoNewline ([char]27 + "]104" + [char]27 + "\\")
Write-Host ([char]27 + "[31m" + "REDAFTER104" + [char]27 + "[0m")
