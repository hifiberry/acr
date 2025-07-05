@echo off
setlocal EnableDelayedExpansion

REM Script to run the AudioControl integration test suite
REM Usage: 
REM   run-test.bat                    - Run all tests
REM   run-test.bat test_name          - Run specific test
REM   run-test.bat test1 test2 test3  - Run multiple specific tests
REM
REM Examples:
REM   run-test.bat test_librespot_api_events
REM   run-test.bat test_librespot_api_events test_generic_player_becomes_active_on_playing

if "%~1"=="" (
    echo [TEST] Running AudioControl Integration Test Suite ^(All Tests^)
    echo =========================================================
    set "TEST_ARGS="
) else (
    echo [TEST] Running AudioControl Integration Test Suite ^(Specific Tests^)
    echo ==============================================================
    echo Tests to run: %*
    echo.
    REM For multiple tests, we need to pass them as space-separated arguments
    REM Rust test filter supports space-separated names
    set "TEST_ARGS=%*"
)

REM Ensure we're in the correct directory
cd /d "%~dp0"

REM Kill any existing audiocontrol processes before starting
echo [CLEANUP] Cleaning up any existing audiocontrol processes...
taskkill /F /IM audiocontrol.exe 2>nul || echo No existing audiocontrol processes found

echo [WAIT] Waiting for process cleanup...
timeout /t 1 /nobreak >nul

REM Run the integration tests with verbose output
echo [START] Starting integration test suite...
echo.

if not defined TEST_ARGS (
    REM Run all tests
    cargo test --test full_integration_tests -- --nocapture
) else (
    REM Run specific tests - for multiple tests, we need to run them individually
    for %%t in (!TEST_ARGS!) do (
        echo Running test: %%t
        cargo test --test full_integration_tests "%%t" -- --nocapture
        if !ERRORLEVEL! neq 0 (
            echo [FAIL] Test %%t failed
            set TEST_EXIT_CODE=1
            goto :post_cleanup
        )
        echo [PASS] Test %%t passed
        echo.
    )
)

REM Capture the exit code
if not defined TEST_ARGS (
    set TEST_EXIT_CODE=%ERRORLEVEL%
) else (
    REM For specific tests, exit code was already set in the loop
    if not defined TEST_EXIT_CODE set TEST_EXIT_CODE=0
)

:post_cleanup
REM Additional cleanup after tests
echo.
echo [CLEANUP] Post-test cleanup...
taskkill /F /IM audiocontrol.exe 2>nul || echo No audiocontrol processes to clean up

REM Clean up test artifacts
del /q test_config_*.json 2>nul || echo No config files to clean up
rmdir /s /q test_cache_* 2>nul || echo No cache directories to clean up

echo [CLEANUP] Cleanup complete
echo.

REM Report results
if %TEST_EXIT_CODE% equ 0 (
    if not defined TEST_ARGS (
        echo [PASS] All integration tests passed!
    ) else (
        echo [PASS] Selected integration tests passed!
    )
) else (
    if not defined TEST_ARGS (
        echo [FAIL] Some integration tests failed ^(exit code: %TEST_EXIT_CODE%^)
    ) else (
        echo [FAIL] Some selected integration tests failed ^(exit code: %TEST_EXIT_CODE%^)
    )
)

echo ==============================================

exit /b %TEST_EXIT_CODE%
