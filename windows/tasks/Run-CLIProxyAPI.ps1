$r='C:\ProgramData\CPAStack'
if(Test-Path "$r\state\cli-proxy-api.disabled"){exit 0}
if(Test-Path "$r\config\proxy.psd1"){(Import-PowerShellDataFile "$r\config\proxy.psd1").GetEnumerator() | % {Set-Item "Env:$($_.Key)" $_.Value}}
& "$r\current\cli-proxy-api\cli-proxy-api.exe" -config "$r\config\config.yaml" *>> "$r\logs\cli-proxy-api.out.log"
