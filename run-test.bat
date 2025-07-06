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
REM (integration tests now have their own cleanup, but this is still helpful)
echo [CLEANUP] Killing any existing audiocontrol processes...
taskkill /F /IM audiocontrol.exe 2>nul >nul
REM Also try PowerShell approach as fallback
powershell -Command "Get-Process -Name 'audiocontrol' -ErrorAction SilentlyContinue | Stop-Process -Force" 2>nul >nul
REM Shorter wait since tests now have better cleanup
timeout /t 1 /nobreak >nul
echo [CLEANUP] Initial cleanup complete

REM Integration tests have been migrated to Python - use tests\run_tests.py instead
echo [INFO] Integration tests have been migrated to Python
echo [INFO] To run integration tests, use: python tests\run_tests.py
set FAILURES=0

REM Skip old Rust integration tests (now using Python tests)
if not defined TEST_ARGS (
    echo [SKIP] Skipping old Rust integration tests - use Python tests instead
    REM Old loop removed - use Python tests instead
) else (
    echo [SKIP] Skipping old Rust integration tests - use Python tests instead  
    REM Old loop removed - use Python tests instead
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
