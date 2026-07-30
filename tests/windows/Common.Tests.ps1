. "$PSScriptRoot\TestHelpers.ps1"
. "$PSScriptRoot\..\..\windows\lib\Common.ps1"
Assert-Throws { Assert-WindowsX64 -OperatingSystem 'Linux' -Is64Bit $true } 'Windows is required'
Assert-Throws { Assert-WindowsX64 -OperatingSystem 'Windows_NT' -Is64Bit $false } 'Windows x64 is required'
$oldRoot=$env:CPA_STACK_ROOT; $env:CPA_STACK_ROOT=New-TestRoot
try { Initialize-CPAStackLayout -SkipAcl | Out-Null; 'config','auths','keeper','releases','current','logs','state','tasks','downloads' | % { Assert-Path (Join-Path $env:CPA_STACK_ROOT $_) 'Container' } } finally { Remove-Item $env:CPA_STACK_ROOT -Recurse -Force; $env:CPA_STACK_ROOT=$oldRoot }
Write-Host 'PASS Common.Tests.ps1'
