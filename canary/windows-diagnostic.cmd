@echo off
setlocal EnableExtensions
rem Native Windows entrypoint. Python owns bounded process supervision and reports.
python "%~dp0actions\windows_diagnostic.py" %*
exit /b %ERRORLEVEL%
