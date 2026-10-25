@echo off
setlocal
pushd "%~dp0.."
call npm run sign:firefox
set "SIGN_RESULT=%ERRORLEVEL%"
popd
exit /b %SIGN_RESULT%
