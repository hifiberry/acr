#!/bin/bash
# Helper script to rebuild just the binary when needed

# Ensure we're running in bash
if [ -z "$BASH_VERSION" ]; then
    echo "Error: This script requires bash to run properly."
    echo "Please run as: bash $0"
    exit 1
fi

set -eo pipefail

# Check if the target binary exists
if [ ! -f "target/release/acr" ]; then
    echo "Binary not found, building with cargo..."
    cargo build --release
    
    if [ ! -f "target/release/acr" ]; then
        echo "ERROR: Failed to build binary!"
        exit 1
    fi
    
    echo "Binary built successfully at target/release/acr"
else
    echo "Binary already exists at target/release/acr"
fi

# Make sure the binary is executable
chmod +x target/release/acr
