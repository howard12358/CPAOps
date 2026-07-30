$ErrorActionPreference = 'Stop'
function Get-CPAStackRoot { if ($env:CPA_STACK_ROOT) { $env:CPA_STACK_ROOT } else { Join-Path $env:ProgramData 'CPAStack' } }
function Assert-WindowsX64 { if (-not $IsWindows -and $env:OS -ne 'Windows_NT') { throw 'Windows is required.' }; if ([Environment]::Is64BitOperatingSystem -ne $true) { throw 'Windows x64 is required.' } }
function Assert-Administrator { $id=[Security.Principal.WindowsIdentity]::GetCurrent(); $p=New-Object Security.Principal.WindowsPrincipal($id); if (-not $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run this command from an elevated Administrator PowerShell.' } }
function Resolve-CPAService([string]$Name) { switch ($Name) { 'cli' {'cli-proxy-api'} 'cli-proxy-api' {'cli-proxy-api'} 'keeper' {'cpa-usage-keeper'} 'cpa-usage-keeper' {'cpa-usage-keeper'} default { throw "Unknown service: $Name" } } }
function Get-CPATaskName([string]$Service) { if ((Resolve-CPAService $Service) -eq 'cli-proxy-api') {'CPAStack-CLIProxyAPI'} else {'CPAStack-UsageKeeper'} }
function Initialize-CPAStackLayout { $r=Get-CPAStackRoot; 'config','auths','keeper','releases','current','logs','state','tasks' | % { New-Item -ItemType Directory -Force -Path (Join-Path $r $_) | Out-Null }; & icacls $r /inheritance:r /grant '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null; $r }
