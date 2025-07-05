//! Integration test suite that runs all other integration test modules

mod full_integration_tests;
mod librespot_integration_tests;
mod activemonitor_integration_test;

// This file is obsolete and not supported by Rust's integration test system. All integration tests are run individually.
// See run-test.bat or test.ps1 for running all tests.
