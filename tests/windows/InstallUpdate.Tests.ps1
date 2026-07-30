. "$PSScriptRoot\TestHelpers.ps1"
. "$PSScriptRoot\..\..\windows\lib\Common.ps1"
Assert-Equal 'cli-proxy-api' (Resolve-CPAService 'cli')
Assert-Equal 'cpa-usage-keeper' (Resolve-CPAService 'keeper')
Assert-Throws { Resolve-CPAService 'all' } 'Unknown service'
Write-Host 'PASS InstallUpdate.Tests.ps1'
