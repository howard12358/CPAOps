. "$PSScriptRoot\TestHelpers.ps1"
. "$PSScriptRoot\..\..\windows\lib\Common.ps1"
. "$PSScriptRoot\..\..\windows\lib\Network.ps1"
$old=$env:CPA_STACK_ROOT; $env:CPA_STACK_ROOT=New-TestRoot
try {
  Initialize-CPAStackLayout -SkipAcl | Out-Null
  Set-CPAStackProxy 'export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897 all_proxy=socks5://127.0.0.1:7897'
  Assert-Path (Get-CPAStackProxyPath) 'Leaf'
  Assert-Equal 'http://127.0.0.1:7897' $env:https_proxy
  Assert-Throws { Set-CPAStackProxy 'export https_proxy=invalid' } 'Invalid proxy format'
} finally {Remove-Item $env:CPA_STACK_ROOT -Recurse -Force; $env:CPA_STACK_ROOT=$old}
Write-Host 'PASS Network.Tests.ps1'
