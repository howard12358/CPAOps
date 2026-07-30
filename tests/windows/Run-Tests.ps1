$ErrorActionPreference = 'Stop'
$root=Split-Path $PSScriptRoot -Parent
$tests=Get-ChildItem $PSScriptRoot -Filter '*.Tests.ps1' | Sort-Object Name
foreach($test in $tests) { & $test.FullName }
Write-Host "PASS $($tests.Count) Windows test file(s)"
