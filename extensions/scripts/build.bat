@echo off
setlocal
pushd "%~dp0.."
call npm run build:all
set "BUILD_RESULT=%ERRORLEVEL%"
popd
exit /b %BUILD_RESULT%
