@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force; if ((Get-ExecutionPolicy -Scope CurrentUser) -ne 'RemoteSigned') { exit 1 }"
if errorlevel 1 echo Unable to set CurrentUser RemoteSigned policy.& exit /b 1
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','RemoteSigned','-File','%~dp0install.ps1'"
