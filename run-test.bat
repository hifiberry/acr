@echo off
setlocal EnableDelayedExpansion

REM Script to run the AudioControl integration test suite
REM Usage: 
REM   run-test.bat                    - Run all tests (normal output)
REM   run-test.bat --quiet            - Only show summary
REM   run-test.bat --verbose          - Show full output
REM   run-test.bat test_name ...      - Run specific test(s)
REM   run-test.bat --quiet test_name  - Quiet, specific test(s)
REM   run-test.bat --verbose test_name - Verbose, specific test(s)
REM
REM Examples:
REM   run-test.bat test_librespot_api_events
REM   run-test.bat --quiet test_librespot_api_events
REM   run-test.bat --verbose test_librespot_api_events

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
taskkill /F /IM audiocontrol.exe 2>nul >nul
REM Wait for process cleanup
timeout /t 1 /nobreak >nul

REM List of test files
set TEST_FILES=full_integration_tests librespot_integration_tests activemonitor_integration_test
set FAILURES=0

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

REM Cleanup
if exist output.txt del /q output.txt
taskkill /F /IM audiocontrol.exe 2>nul >nul
del /q test_config_*.json 2>nul >nul
rmdir /s /q test_cache_* 2>nul >nul

REM Report results
if %FAILURES%==0 (
    echo [PASS] All integration tests passed!
    exit /b 0
) else (
    echo [FAIL] Some integration tests failed!
    exit /b 1
)
