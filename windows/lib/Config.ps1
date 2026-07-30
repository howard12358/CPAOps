. "$PSScriptRoot\Common.ps1"
function Get-CPAConfigPath { Join-Path (Get-CPAStackRoot) 'config\config.yaml' }
function Get-KeeperEnvPath { Join-Path (Get-CPAStackRoot) 'config\keeper.env' }
function Initialize-PrivateAcl {
  param([string]$Path)
  & icacls $Path /inheritance:r /grant '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
}
function Read-Secret([string]$Prompt) { $s=Read-Host $Prompt -AsSecureString; [System.Net.NetworkCredential]::new('', $s).Password }
function Initialize-CPAConfig {
  param([string]$RepoRoot)
  $r=Get-CPAStackRoot; $c=Get-CPAConfigPath; $k=Get-KeeperEnvPath
  if (!(Test-Path $c) -or !(Test-Path $k)) { $key=Read-Secret 'CPA management key'; if (!(Test-Path $c)) {(Get-Content "$RepoRoot\config\cpa.config.yaml.example" -Raw).Replace('__REQUIRED__',$key) | Set-Content $c -NoNewline}; if (!(Test-Path $k)) {$pwd=Read-Secret 'Keeper login password'; (Get-Content "$RepoRoot\config\keeper.env.example" -Raw).Replace('__REQUIRED__',$key).Replace('LOGIN_PASSWORD=__REQUIRED__',"LOGIN_PASSWORD=$pwd") | Set-Content $k -NoNewline} }
  if (!(Select-String -Path $k -Pattern '^CPA_PUBLIC_URL=' -Quiet)) { Add-Content $k 'CPA_PUBLIC_URL=http://127.0.0.1:8317' }
  if ((Get-Content $c -Raw) -match '__REQUIRED__' -or (Get-Content $k -Raw) -match '__REQUIRED__') { throw 'Configuration contains required placeholders.' }
  Initialize-PrivateAcl (Join-Path $r 'config')
}
