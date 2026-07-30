param([string]$Service='all'); & "$PSScriptRoot\stop.ps1" $Service; & "$PSScriptRoot\start.ps1" $Service
