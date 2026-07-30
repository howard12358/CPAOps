$ErrorActionPreference = 'Stop'
function Get-CPAStackRoot { if ($env:CPA_STACK_ROOT) { $env:CPA_STACK_ROOT } else { Join-Path $env:ProgramData 'CPAStack' } }
function Assert-WindowsX64 {
  param([string]$OperatingSystem = $env:OS, [bool]$Is64Bit = [Environment]::Is64BitOperatingSystem)
  if ($OperatingSystem -ne 'Windows_NT') { throw 'Windows is required.' }
  if (-not $Is64Bit) { throw 'Windows x64 is required.' }
}
function Assert-Administrator {
  param([Nullable[bool]]$IsElevated)
  if ($null -eq $IsElevated) { $id=[Security.Principal.WindowsIdentity]::GetCurrent(); $p=New-Object Security.Principal.WindowsPrincipal($id); $IsElevated=$p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) }
  if (-not $IsElevated) { throw 'Run this command from an elevated Administrator PowerShell.' }
}
function Resolve-CPAService([string]$Name) { switch ($Name) { 'cli' {'cli-proxy-api'} 'cli-proxy-api' {'cli-proxy-api'} 'keeper' {'cpa-usage-keeper'} 'cpa-usage-keeper' {'cpa-usage-keeper'} default { throw "Unknown service: $Name" } } }
function Get-CPATaskName([string]$Service) { if ((Resolve-CPAService $Service) -eq 'cli-proxy-api') {'CPAStack-CLIProxyAPI'} else {'CPAStack-UsageKeeper'} }
function Initialize-CPAStackLayout {
  param([switch]$SkipAcl)
  $r=Get-CPAStackRoot
  'config','auths','keeper','releases','current','logs','state','tasks','downloads' | % { New-Item -ItemType Directory -Force -Path (Join-Path $r $_) | Out-Null }
  if (-not $SkipAcl) { & icacls $r /inheritance:r /grant '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null }
  $r
}
