. "$PSScriptRoot\Common.ps1"
function Set-CPAStackFirewall {
  param([switch]$Remove)
  $name='CPAStack-Block-Remote-Keeper'
  if($Remove){Remove-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue; return}
  Remove-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue
  New-NetFirewallRule -DisplayName $name -Direction Inbound -Action Block -Protocol TCP -LocalPort 18080 -Profile Any | Out-Null
}
