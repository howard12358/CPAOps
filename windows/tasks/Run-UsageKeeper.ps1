$r='C:\ProgramData\CPAStack'
if(Test-Path "$r\state\cpa-usage-keeper.disabled"){exit 0}
if(Test-Path "$r\config\proxy.psd1"){(Import-PowerShellDataFile "$r\config\proxy.psd1").GetEnumerator() | % {Set-Item "Env:$($_.Key)" $_.Value}}
& "$r\current\cpa-usage-keeper\cpa-usage-keeper.exe" -env "$r\config\keeper.env" *>> "$r\logs\cpa-usage-keeper.out.log"
