@echo off
node "%~dp0build-wasm.mjs" %*
exit /b %ERRORLEVEL%
