@echo off
setlocal EnableDelayedExpansion

REM Script to run the AudioControl full test suite (integration + unit tests)
REM Usage:
REM   run-test.bat [--quiet|--verbose] [test_name ...]
REM
REM Runs all integration tests and the full unit test suite.

REM Parse --quiet/--verbose flags
set QUIET=0
set VERBOSE=0
set TEST_ARGS=
:parse_args
if "%~1"=="" goto after_args
if "%~1"=="--quiet" (
    set QUIET=1
) else if "%~1"=="--verbose" (
    set VERBOSE=1
) else (
    if defined TEST_ARGS (
        set TEST_ARGS=!TEST_ARGS! %1
    ) else (
        set TEST_ARGS=%1
    )
)
shift
goto parse_args
:after_args

REM Ensure we're in the correct directory
cd /d "%~dp0"

REM Kill any existing audiocontrol processes before starting
REM (integration tests may start/stop the server)
echo [CLEANUP] Killing any existing audiocontrol processes...
taskkill /F /IM audiocontrol.exe 2>nul >nul
REM Also try PowerShell approach as fallback
powershell -Command "Get-Process -Name 'audiocontrol' -ErrorAction SilentlyContinue | Stop-Process -Force" 2>nul >nul
REM Wait for processes to fully terminate
timeout /t 2 /nobreak >nul
echo [CLEANUP] Process cleanup complete

REM List of integration test files
set TEST_FILES=generic_integration_tests librespot_integration_tests activemonitor_integration_test raat_integration_tests mpd_integration_tests cli_integration_tests
set FAILURES=0

REM Run integration tests
if not defined TEST_ARGS (
    for %%f in (%TEST_FILES%) do (
        echo [RUN] %%f
        if %QUIET%==1 (
            cargo test --test %%f -- --quiet > output.txt 2>&1
        ) else if %VERBOSE%==1 (
            cargo test --test %%f -- --nocapture
        ) else (
            cargo test --test %%f
        )
        if !ERRORLEVEL! neq 0 (
            echo [FAIL] %%f
            set /a FAILURES+=1
            if %QUIET%==1 (
                type output.txt
            )
        ) else (
            echo [PASS] %%f
        )
    )
) else (
    for %%f in (%TEST_FILES%) do (
        for %%t in (!TEST_ARGS!) do (
            echo [RUN] %%f %%t
            if %QUIET%==1 (
                cargo test --test %%f %%t -- --quiet > output.txt 2>&1
            ) else if %VERBOSE%==1 (
                cargo test --test %%f %%t -- --nocapture
            ) else (
                cargo test --test %%f %%t
            )
            if !ERRORLEVEL! neq 0 (
                echo [FAIL] %%f %%t
                set /a FAILURES+=1
                if %QUIET%==1 (
                    type output.txt
                )
            ) else (
                echo [PASS] %%f %%t
            )
        )
    )
)

REM Run all unit tests (main crate)
echo [RUN] crate unit tests
if %QUIET%==1 (
    cargo test --lib -- --quiet > output.txt 2>&1
) else if %VERBOSE%==1 (
    cargo test --lib -- --nocapture
) else (
    cargo test --lib
)
if !ERRORLEVEL! neq 0 (
    echo [FAIL] crate unit tests
    set /a FAILURES+=1
    if %QUIET%==1 (
        type output.txt
    )
) else (
    echo [PASS] crate unit tests
)

REM Cleanup
echo [CLEANUP] Cleaning up test artifacts...
if exist output.txt del /q output.txt
REM Kill any remaining audiocontrol processes
taskkill /F /IM audiocontrol.exe 2>nul >nul
powershell -Command "Get-Process -Name 'audiocontrol' -ErrorAction SilentlyContinue | Stop-Process -Force" 2>nul >nul
REM Clean up test config files and cache directories
del /q test_config_*.json 2>nul >nul
rmdir /s /q test_cache_* 2>nul >nul
echo [CLEANUP] Cleanup complete

REM Report results
if %FAILURES%==0 (
    echo [PASS] All integration and unit tests passed!
    exit /b 0
) else (
    echo [FAIL] Some tests failed!
    exit /b 1
)
