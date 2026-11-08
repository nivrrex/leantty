@echo off
powershell.exe -NoProfile -Command "Get-NetFirewallRule -DisplayName 'W2_BLOCK_*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule -Confirm:$false"
