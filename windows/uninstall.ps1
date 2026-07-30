param([switch]$Purge)
. "$PSScriptRoot\lib\Common.ps1"
Assert-Administrator
'CPAStack-CLIProxyAPI','CPAStack-UsageKeeper' | % { Unregister-ScheduledTask -TaskName $_ -Confirm:$false -ErrorAction SilentlyContinue }
if($Purge){$answer=Read-Host "Type DELETE to remove $(Get-CPAStackRoot)"; if($answer -ne 'DELETE'){throw 'Cancelled.'}; Remove-Item (Get-CPAStackRoot) -Recurse -Force}
