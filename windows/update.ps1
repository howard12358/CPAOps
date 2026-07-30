param(
  [Parameter(Position=0)][ValidateSet('all','cli','keeper')][string]$Service='all',
  [switch]$Rollback,
  [ValidateSet('cli','keeper')][string]$RollbackService,
  [string]$Version
)
. "$PSScriptRoot\lib\Common.ps1"
. "$PSScriptRoot\lib\GitHubRelease.ps1"
. "$PSScriptRoot\lib\ScheduledTask.ps1"
Assert-Administrator
function Backup-KeeperDatabase {
  $root=Get-CPAStackRoot; $source=Join-Path $root 'keeper'; $target=Join-Path $source ("backups\pre-update-"+(Get-Date -Format 'yyyyMMdd-HHmmss'))
  New-Item -ItemType Directory -Force $target | Out-Null
  'app.db','app.db-wal','app.db-shm' | % { $file=Join-Path $source $_; if(Test-Path $file){Copy-Item $file $target -Force} }
  $target
}
if($Rollback) {
  if(!$RollbackService -or !$Version){throw 'Rollback requires -RollbackService cli|keeper and -Version <version>.'}
  $current=Get-CurrentReleaseVersion $RollbackService; if($current -eq $Version){Write-Host "$(Resolve-CPAService $RollbackService) is already $Version."; exit 0}
  Stop-CPAStackService $RollbackService; Set-CurrentRelease $RollbackService $Version; Start-CPAStackService $RollbackService; Write-Host "Rolled back $RollbackService to $Version."; exit 0
}
$services=if($Service -eq 'all'){'cli','keeper'}else{@($Service)}
foreach($s in $services) {
  Write-Host "Checking latest $s release..."
  Stop-CPAStackService $s
  if($s -eq 'keeper'){Write-Host "Keeper backup: $(Backup-KeeperDatabase)"}
  try { $v=Install-VerifiedRelease $s; Start-CPAStackService $s; Write-Host "$s $v is active." }
  catch { try {Restore-PreviousRelease $s; Start-CPAStackService $s} catch {}; throw }
}
