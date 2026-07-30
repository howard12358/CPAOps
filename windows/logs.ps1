param([Parameter(Position=0, Mandatory=$true)][ValidateSet('cli','keeper')][string]$Service)
. "$PSScriptRoot\lib\Common.ps1"
$prefix=if($Service -eq 'cli'){'cli-proxy-api'}else{'cpa-usage-keeper'}
$logs=Join-Path (Get-CPAStackRoot) 'logs'; New-Item -ItemType Directory -Force $logs | Out-Null
$out=Join-Path $logs "$prefix.out.log"; $err=Join-Path $logs "$prefix.err.log"; New-Item -ItemType File -Force $out,$err | Out-Null
Get-Content $out,$err -Tail 200 -Wait
