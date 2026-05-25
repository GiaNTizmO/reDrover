@echo off
rem Convenience wrapper around scripts\build.ps1 for users who'd rather
rem double-click or run from cmd.exe. Forwards all arguments verbatim.
rem
rem Examples:
rem     scripts\build.cmd
rem     scripts\build.cmd -Config Debug
rem     scripts\build.cmd -Clean

setlocal
set "SCRIPT_DIR=%~dp0"

where pwsh >nul 2>nul
if %ERRORLEVEL%==0 (
    pwsh -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%build.ps1" %*
    goto :end
)

where powershell >nul 2>nul
if %ERRORLEVEL%==0 (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%build.ps1" %*
    goto :end
)

echo Neither pwsh nor powershell is on PATH. Install PowerShell 7 or use Windows PowerShell.
exit /b 1

:end
endlocal
exit /b %ERRORLEVEL%
