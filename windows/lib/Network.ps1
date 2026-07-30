. "$PSScriptRoot\Common.ps1"
function Get-CPAStackProxyPath { Join-Path (Get-CPAStackRoot) 'config\proxy.psd1' }
function Get-CPAStackTokenPath { Join-Path (Get-CPAStackRoot) 'config\github-token' }
function Import-CPAStackProxy { $p=Get-CPAStackProxyPath; if(Test-Path $p){$v=Import-PowerShellDataFile $p; foreach($n in 'https_proxy','http_proxy','all_proxy'){if($v.ContainsKey($n)){Set-Item "Env:$n" $v[$n]}}} }
function Set-CPAStackProxy([string]$Line) {
  if(!$Line){$Line=Read-Host 'Optional proxy (export/set format, blank to skip)'}; if(!$Line){return}
  $values=@{}; [regex]::Matches($Line,'(?i)(https?_proxy|all_proxy)=([^\s]+)') | % {$values[$_.Groups[1].Value.ToLowerInvariant()]=$_.Groups[2].Value}
  if(!$values.Count -or @($values.Values|? {$_ -notmatch '^(http|https|socks5)://[^\s]+$'}).Count){throw 'Invalid proxy format.'}
  $body="@{`n" + (($values.Keys|Sort-Object|% {"    '$($_)' = '$($values[$_].Replace("'","''"))'"}) -join "`n") + "`n}`n"
  Set-Content (Get-CPAStackProxyPath) $body -NoNewline; Import-CPAStackProxy
}
function Invoke-CPAStackWeb {
  param([string]$Uri,[string]$OutFile,[scriptblock]$Request)
  Import-CPAStackProxy
  if(!$Request){$Request={param($Headers) Invoke-WebRequest $Uri -OutFile $OutFile -UseBasicParsing -Headers $Headers}}
  try { & $Request @{}; return } catch {$code=[int]$_.Exception.Response.StatusCode.value__; if($code -notin 401,403){throw}}
  $tokenPath=Get-CPAStackTokenPath; $token=if(Test-Path $tokenPath){(Get-Content $tokenPath -Raw).Trim()}else{Read-Host 'GitHub token'}
  if(!$token){throw 'GitHub access denied and no token supplied.'}; Set-Content $tokenPath $token -NoNewline; & icacls $tokenPath /inheritance:r /grant '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
  & $Request @{Authorization="Bearer $token"}
}
