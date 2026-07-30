. "$PSScriptRoot\TestHelpers.ps1"
. "$PSScriptRoot\..\..\windows\lib\Common.ps1"
. "$PSScriptRoot\..\..\windows\lib\Config.ps1"
$old=$env:CPA_STACK_ROOT; $env:CPA_STACK_ROOT=New-TestRoot
try { Initialize-CPAStackLayout -SkipAcl|Out-Null; Set-Content (Get-CPAConfigPath) 'secret-key: __REQUIRED__'; Set-Content (Get-KeeperEnvPath) 'APP_PORT=18080'; Assert-Throws { Initialize-CPAConfig 'missing' } 'CPA management key' } finally {Remove-Item $env:CPA_STACK_ROOT -Recurse -Force; $env:CPA_STACK_ROOT=$old}
Write-Host 'PASS Config.Tests.ps1'
