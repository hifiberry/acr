# PowerShell script to run AudioControl integration tests
# Usage:
#   .\test.ps1                # Run all tests, summary only
#   .\test.ps1 --verbose      # Run all tests, show full output
#   .\test.ps1 testname ...   # Run specific tests, summary only
#   .\test.ps1 --verbose testname ...

param(
    [Parameter(ValueFromRemainingArguments=$true)]
    [string[]]$Args
)

$VerboseMode = $false
$TestArgs = @()

foreach ($arg in $Args) {
    if ($arg -eq '--verbose') {
        $VerboseMode = $true
    } else {
        $TestArgs += $arg
    }
}

Write-Host "[TEST] Running AudioControl Integration Test Suite"
Write-Host "================================================="

# Kill any existing audiocontrol processes
Write-Host "[CLEANUP] Cleaning up any existing audiocontrol processes..."
try { Stop-Process -Name audiocontrol -Force -ErrorAction SilentlyContinue } catch {}
Start-Sleep -Seconds 1

$TestFiles = @(
    'full_integration_tests',
    'librespot_integration_tests',
    'activemonitor_integration_test'
)

$Failures = 0

if ($TestArgs.Count -eq 0) {
    foreach ($test in $TestFiles) {
        Write-Host "[RUN] $test"
        $cmd = "cargo test --test $test --no-fail-fast"
        if (-not $VerboseMode) { $cmd += ' --quiet' }
        $proc = Start-Process -FilePath "cmd.exe" -ArgumentList "/c $cmd" -NoNewWindow -Wait -PassThru -RedirectStandardOutput output.txt -RedirectStandardError error.txt
        $exit = $proc.ExitCode
        if ($VerboseMode) {
            Get-Content output.txt
            Get-Content error.txt
        }
        if ($exit -eq 0) {
            Write-Host "[PASS] $test" -ForegroundColor Green
        } else {
            Write-Host "[FAIL] $test" -ForegroundColor Red
            $Failures++
            if (-not $VerboseMode) {
                Write-Host "[FAILURE OUTPUT]"
                Get-Content output.txt
                Get-Content error.txt
            }
        }
    }
} else {
    foreach ($test in $TestFiles) {
        foreach ($t in $TestArgs) {
            Write-Host "[RUN] $test $t"
            $cmd = "cargo test --test $test $t --no-fail-fast"
            if (-not $VerboseMode) { $cmd += ' --quiet' }
            $proc = Start-Process -FilePath "cmd.exe" -ArgumentList "/c $cmd" -NoNewWindow -Wait -PassThru -RedirectStandardOutput output.txt -RedirectStandardError error.txt
            $exit = $proc.ExitCode
            if ($VerboseMode) {
                Get-Content output.txt
                Get-Content error.txt
            }
            if ($exit -eq 0) {
                Write-Host "[PASS] $test $t" -ForegroundColor Green
            } else {
                Write-Host "[FAIL] $test $t" -ForegroundColor Red
                $Failures++
                if (-not $VerboseMode) {
                    Write-Host "[FAILURE OUTPUT]"
                    Get-Content output.txt
                    Get-Content error.txt
                }
            }
        }
    }
}

# Cleanup
Remove-Item output.txt,error.txt -ErrorAction SilentlyContinue
try { Stop-Process -Name audiocontrol -Force -ErrorAction SilentlyContinue } catch {}
Remove-Item test_config_*.json -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force test_cache_* -ErrorAction SilentlyContinue

if ($Failures -eq 0) {
    Write-Host "[PASS] All integration tests passed!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "[FAIL] Some integration tests failed!" -ForegroundColor Red
    exit 1
}
