@echo off
node "%~dp0generate-release-artifacts.mjs" %*
exit /b %ERRORLEVEL%