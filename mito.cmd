@echo off
setlocal
if "%~1"=="" (
  "%~dp0core\bin\mito.exe" start
) else (
  "%~dp0core\bin\mito.exe" %*
)
exit /b %errorlevel%
