. "$PSScriptRoot\lib\Common.ps1"
$root=Get-CPAStackRoot
if(!(Test-Path $root -PathType Container)){throw "CPAStack runtime directory does not exist: $root"}
if(!$env:WT_SESSION -or !(Get-Command wt.exe -ErrorAction SilentlyContinue)){throw 'Run this command inside Windows Terminal; it opens a tab in the current terminal window.'}
& wt.exe -w 0 new-tab -d $root
