@echo off
setlocal

if "%~1"=="" goto :usage
if "%~2"=="" goto :usage
set "RUNTIME_VERSION=%~1"
set "CLI_VERSION=%~2"
set "GRAMMAR_REF=%~3"
if not defined GRAMMAR_REF set "GRAMMAR_REF=main"
for %%I in ("%~dp0..") do set "PROJECT_ROOT=%%~fI"
set "GRAMMAR_DIR=%PROJECT_ROOT%\grammar"
set "GRAMMAR_URL=https://codeberg.org/grammar-orchard/tree-sitter-json5-orchard.git"
set "TEMP_ROOT=%TEMP%\gpui-json5-grammar-%RANDOM%-%RANDOM%"

where git >nul 2>nul || (echo Git was not found on PATH. 1>&2 & exit /b 1)
where cargo >nul 2>nul || (echo Cargo was not found on PATH. Install Rust first. 1>&2 & exit /b 1)
set "INSTALLED_CLI_VERSION="
where tree-sitter >nul 2>nul
if not errorlevel 1 for /f "tokens=2" %%V in ('tree-sitter --version 2^>nul') do set "INSTALLED_CLI_VERSION=%%V"
if not "%INSTALLED_CLI_VERSION%"=="%CLI_VERSION%" (
    cargo install tree-sitter-cli --version "%CLI_VERSION%" --locked --force
    if errorlevel 1 goto :failure
    if defined CARGO_HOME (set "PATH=%CARGO_HOME%\bin;%PATH%") else set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)
set "INSTALLED_CLI_VERSION="
for /f "tokens=2" %%V in ('tree-sitter --version 2^>nul') do set "INSTALLED_CLI_VERSION=%%V"
if not "%INSTALLED_CLI_VERSION%"=="%CLI_VERSION%" (
    echo Expected Tree-sitter CLI %CLI_VERSION%, found %INSTALLED_CLI_VERSION%. 1>&2
    goto :failure
)

mkdir "%TEMP_ROOT%" || goto :failure
git clone "%GRAMMAR_URL%" "%TEMP_ROOT%\upstream"
if errorlevel 1 goto :failure
git -C "%TEMP_ROOT%\upstream" fetch origin "%GRAMMAR_REF%"
if errorlevel 1 goto :failure
git -C "%TEMP_ROOT%\upstream" checkout --detach FETCH_HEAD
if errorlevel 1 goto :failure
pushd "%TEMP_ROOT%\upstream"
tree-sitter generate
if errorlevel 1 (popd & goto :failure)
popd

if not exist "%TEMP_ROOT%\upstream\src\parser.c" goto :missing_parser
if not exist "%TEMP_ROOT%\upstream\src\node-types.json" goto :missing_node_types
if not exist "%TEMP_ROOT%\upstream\src\tree_sitter\parser.h" goto :missing_parser_header
if not exist "%TEMP_ROOT%\upstream\queries\highlights.scm" goto :missing_highlights
findstr /c:"tree_sitter_json5_orchard" "%TEMP_ROOT%\upstream\src\parser.c" >nul
if errorlevel 1 goto :wrong_symbol

pushd "%PROJECT_ROOT%"
cargo add "tree-sitter@=%RUNTIME_VERSION%" --package gpui-json5-processor
if errorlevel 1 (popd & goto :failure)
cargo update -p tree-sitter --precise "%RUNTIME_VERSION%"
if errorlevel 1 (popd & goto :failure)
popd

copy /y "%TEMP_ROOT%\upstream\src\parser.c" "%GRAMMAR_DIR%\src\parser.c" >nul || goto :failure
copy /y "%TEMP_ROOT%\upstream\src\node-types.json" "%GRAMMAR_DIR%\src\node-types.json" >nul || goto :failure
if exist "%GRAMMAR_DIR%\src\scanner.c" del /q "%GRAMMAR_DIR%\src\scanner.c"
if exist "%TEMP_ROOT%\upstream\src\scanner.c" (
    copy /y "%TEMP_ROOT%\upstream\src\scanner.c" "%GRAMMAR_DIR%\src\scanner.c" >nul
    if errorlevel 1 goto :failure
)
if exist "%GRAMMAR_DIR%\src\tree_sitter" rmdir /s /q "%GRAMMAR_DIR%\src\tree_sitter"
mkdir "%GRAMMAR_DIR%\src\tree_sitter" || goto :failure
if exist "%TEMP_ROOT%\upstream\src\tree_sitter" xcopy "%TEMP_ROOT%\upstream\src\tree_sitter\*" "%GRAMMAR_DIR%\src\tree_sitter\" /e /i /y >nul
if errorlevel 1 goto :failure
if exist "%GRAMMAR_DIR%\queries" rmdir /s /q "%GRAMMAR_DIR%\queries"
mkdir "%GRAMMAR_DIR%\queries" || goto :failure
xcopy "%TEMP_ROOT%\upstream\queries\*" "%GRAMMAR_DIR%\queries\" /e /i /y >nul
if errorlevel 1 goto :failure
for %%F in (grammar.js tree-sitter.json) do if exist "%TEMP_ROOT%\upstream\%%F" copy /y "%TEMP_ROOT%\upstream\%%F" "%GRAMMAR_DIR%\%%F" >nul
if exist "%TEMP_ROOT%\upstream\src\grammar.json" copy /y "%TEMP_ROOT%\upstream\src\grammar.json" "%GRAMMAR_DIR%\src\grammar.json" >nul
if exist "%TEMP_ROOT%\upstream\LICENSE" copy /y "%TEMP_ROOT%\upstream\LICENSE" "%GRAMMAR_DIR%\LICENSE" >nul
git -C "%TEMP_ROOT%\upstream" rev-parse HEAD > "%GRAMMAR_DIR%\UPSTREAM_REVISION"
if errorlevel 1 goto :failure

rmdir /s /q "%TEMP_ROOT%"
echo Updated Tree-sitter runtime to %RUNTIME_VERSION% and JSON5 grammar to %GRAMMAR_REF%.
echo Next, run npm run build:wasm from the www directory to verify WASM compatibility.
exit /b 0

:usage
echo Usage: %~nx0 ^<runtime-version^> ^<tree-sitter-cli-version^> [grammar-ref] 1>&2
echo The grammar ref defaults to main. 1>&2
exit /b 2
:missing_parser
echo The selected grammar revision did not generate src/parser.c. 1>&2
goto :failure
:missing_node_types
echo The selected grammar revision did not generate src/node-types.json. 1>&2
goto :failure
:missing_parser_header
echo The selected grammar revision did not generate src/tree_sitter/parser.h. 1>&2
goto :failure
:missing_highlights
echo The selected grammar revision has no queries/highlights.scm. 1>&2
goto :failure
:wrong_symbol
echo The generated parser does not export tree_sitter_json5_orchard. 1>&2
goto :failure
:failure
if exist "%TEMP_ROOT%" rmdir /s /q "%TEMP_ROOT%"
echo Tree-sitter update failed. Cargo.toml or Cargo.lock may already have been updated. 1>&2
exit /b 1
