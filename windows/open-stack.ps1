. "$PSScriptRoot\lib\Common.ps1"
$root=Get-CPAStackRoot
if(!(Test-Path $root -PathType Container)){throw "CPAStack runtime directory does not exist: $root"}
Start-Process powershell.exe -WorkingDirectory $root
