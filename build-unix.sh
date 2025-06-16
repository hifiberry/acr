#!/bin/bash
# Script to prepare and build the acr package on a Unix/Linux system

set -eo pipefail

echo "===== Preparing ACR for Unix Build ====="

# Ensure all files have Unix line endings
echo "Fixing line endings in key files..."
for file in $(find debian -type f) build.sh Cargo.toml; do
    if [ -f "$file" ]; then
        echo "Checking $file..."
        if grep -q $'\r' "$file" 2>/dev/null; then
            echo "Converting $file to Unix line endings..."
            tr -d '\r' < "$file" > "${file}.unix"
            mv "${file}.unix" "$file"
        fi
    fi
done

# Ensure scripts are executable
echo "Making scripts executable..."
chmod +x build.sh
chmod +x debian/rules debian/postinst debian/preinst

# Create a minimal build.env file if needed
if [ ! -f "build.env" ]; then
    echo "Creating build.env file..."
    cat > build.env << EOF
# ACR build environment variables
SKIP_BUILD=0
EOF
fi

echo "===== Calling main build script ====="
# Add the --force flag if secrets.txt doesn't exist
if [ ! -f "secrets.txt" ]; then
    echo "No secrets.txt found, adding --force flag..."
    ./build.sh --force
else
    ./build.sh
fi

echo "===== Unix build preparation complete ====="
