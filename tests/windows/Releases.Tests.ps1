. "$PSScriptRoot\TestHelpers.ps1"
. "$PSScriptRoot\..\..\windows\lib\Common.ps1"
. "$PSScriptRoot\..\..\windows\lib\GitHubRelease.ps1"
$old=$env:CPA_STACK_ROOT; $env:CPA_STACK_ROOT=New-TestRoot
try {
  Initialize-CPAStackLayout -SkipAcl | Out-Null
  $first=Join-Path $env:CPA_STACK_ROOT 'releases\cli-proxy-api\1.0.0'; $next=Join-Path $env:CPA_STACK_ROOT 'releases\cli-proxy-api\1.1.0'
  New-Item -ItemType Directory -Force $first,$next | Out-Null
  Set-CurrentRelease -Service cli -Version '1.0.0'
  Assert-Equal '1.0.0' (Get-CurrentReleaseVersion cli)
  Set-CurrentRelease -Service cli -Version '1.1.0'
  Assert-Equal '1.1.0' (Get-CurrentReleaseVersion cli)
  Restore-PreviousRelease cli
  Assert-Equal '1.0.0' (Get-CurrentReleaseVersion cli)
} finally {Remove-Item $env:CPA_STACK_ROOT -Recurse -Force; $env:CPA_STACK_ROOT=$old}
Write-Host 'PASS Releases.Tests.ps1'
