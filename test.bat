@echo off
REM Wrapper script to call run-test.bat for legacy/test runner compatibility
REM Usage: test.bat [args]

REM Pass all arguments to run-test.bat
call "%~dp0run-test.bat" %*
