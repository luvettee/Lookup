@echo off
setlocal
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup.ps1"
if %ERRORLEVEL% neq 0 (
    echo.
    echo Setup failed with error code %ERRORLEVEL%.
    pause
    exit /b %ERRORLEVEL%
)
endlocal
