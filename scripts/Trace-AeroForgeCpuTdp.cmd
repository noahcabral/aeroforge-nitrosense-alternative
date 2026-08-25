@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Trace-AeroForgeCpuTdp.ps1" %*
echo.
pause
