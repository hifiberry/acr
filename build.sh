#!/bin/bash
# Ensure we're running in bash
if [ -z "$BASH_VERSION" ]; then
    echo "Error: This script requires bash to run properly."
    echo "Please run as: bash $0"
    exit 1
fi

set -eo pipefail

echo "===== Building ACR Debian Package ====="

# Check Rust version and inform if upgrade is needed
REQUIRED_RUST_VERSION="1.70.0"
CURRENT_RUST_VERSION=$(rustc --version | cut -d ' ' -f 2)

echo "Current Rust version: $CURRENT_RUST_VERSION"
echo "Required Rust version: $REQUIRED_RUST_VERSION"

# Check if secrets.txt exists
if [ ! -f "secrets.txt" ]; then
    if [[ " $* " == *" --force "* ]]; then
        echo "Warning: secrets.txt not found, but continuing due to --force flag"
    else
        echo "Error: secrets.txt not found. This file is required for building."
        echo "You can copy secrets.txt.sample to secrets.txt and modify it,"
        echo "or use --force to build without it."
        exit 1
    fi
fi

# Function to compare version strings
version_lt() {
    # Check if version1 is less than version2
    [ "$(printf '%s\n' "$1" "$2" | sort -V | head -n1)" = "$1" ] && [ "$1" != "$2" ]
}

if version_lt "$CURRENT_RUST_VERSION" "$REQUIRED_RUST_VERSION"; then
    echo "Rust version is too old. This project requires Rust $REQUIRED_RUST_VERSION or later."
    echo ""
    echo "===== Rust Upgrade Instructions ====="
    echo "To upgrade Rust using rustup, run the following commands:"
    echo ""
    echo "If rustup is already installed:"
    echo "    rustup update stable"
    echo ""
    echo "If rustup is not installed:"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "    source \$HOME/.cargo/env"
    echo ""
    echo "Alternatively, you can downgrade the mio dependency instead:"
    echo "    cargo update -p mio@1.0.3 --precise 0.8.8"
    echo ""
    echo "After upgrading Rust or downgrading the dependency, run this script again."
    exit 1
fi

# Create out directory for package files
mkdir -p out

# Update executable name in debian/rules file if it's still set to placeholder
sed -i 's/your_executable_name/acr/g' debian/rules

# Remove debian/compat file as it conflicts with debhelper-compat in control file
if [ -f debian/compat ]; then
    echo "Removing debian/compat file to avoid compatibility level conflict"
    rm debian/compat
fi

# We'll let dh_install handle the directory creation

# Make sure dependencies are installed
command -v cargo >/dev/null 2>&1 || { echo "Cargo is required but not installed. Aborting."; exit 1; }
command -v dpkg-buildpackage >/dev/null 2>&1 || { echo "dpkg-buildpackage is required but not installed. Aborting."; exit 1; }

# Update version in control file from Cargo.toml
VERSION=$(grep -m 1 '^version' Cargo.toml | cut -d'"' -f2)
sed -i "s/^Version:.*/Version: $VERSION/" debian/control

# Check if we want to skip rebuilding the binary
if [ "$1" = "--skip-build" ] || [ "${SKIP_BUILD:-0}" = "1" ]; then
    echo "===== Skipping build stage and reusing existing binaries ====="
    export SKIP_BUILD=1
    
    # Check if the main binary exists
    if [ ! -f "target/release/acr" ]; then
        echo "ERROR: Cannot skip build, main binary not found at target/release/acr"
        echo "       Please run the build without --skip-build first"
        exit 1
    fi
    
    # Check if the CLI tool binaries exist
    for tool in acr_dumpcache acr_lms_client acr_send_update; do
        if [ ! -f "target/release/$tool" ]; then
            echo "WARNING: CLI tool $tool not found at target/release/$tool"
            echo "         Some CLI tools may not be included in the package"
        else
            # Make sure the tool binary is marked as executable
            chmod +x "target/release/$tool"
        fi
    done
      # Make sure the main binary is marked as executable
    chmod +x target/release/acr
else
    echo "===== Building binaries ====="
    echo "Building main application and CLI tools (acr_dumpcache, acr_lms_client, acr_send_update)"
    # Tip: You can use SKIP_BUILD=1 or --skip-build to skip this step next time
    # Clear the SKIP_BUILD variable to ensure a full build
    unset SKIP_BUILD
    export SKIP_BUILD=0
    
    # We'll let dpkg-buildpackage handle the actual build to avoid building twice
fi

# Ensure that dpkg-buildpackage sees our SKIP_BUILD setting
echo "Build mode: SKIP_BUILD=${SKIP_BUILD:-0}"

echo "===== Preparing Debian package ====="

# Make sure the target directory exists
mkdir -p target/release

# We'll let dh_install handle the file copying
echo "Configuration files will be handled by dh_install..."

# Ensure all debian scripts are executable
echo "Making scripts executable..."
chmod +x debian/postinst debian/preinst debian/rules

# Verify that scripts were made executable
for script in postinst preinst rules; do
    if [ ! -x "debian/$script" ]; then
        echo "WARNING: Failed to make $script script executable!"
        # Force it to be executable
        chmod 755 "debian/$script"
    fi
done

# Check if debian/rules has DOS line endings and fix
if grep -q $'\r' debian/rules; then
    echo "Fixing DOS line endings in debian/rules..."
    tr -d '\r' < debian/rules > debian/rules.unix
    mv debian/rules.unix debian/rules
    chmod +x debian/rules
fi

# Create the Debian package
# Pass environment variables explicitly to dpkg-buildpackage
echo "Starting build with SKIP_BUILD=${SKIP_BUILD}"
export SKIP_BUILD
dpkg-buildpackage -us -uc -b

echo "===== Moving package files to out directory ====="
# Move the .deb package file to the out directory
mv ../acr_${VERSION}_*.deb out/
# If there are any additional Debian files created in the parent directory, move them too
mv ../acr_${VERSION}* out/ 2>/dev/null || true

echo "===== Cleaning up ====="
rm ../acr-dbgsym*.deb
# The package will be created in the out directory
echo "Debian package created at: out/acr_${VERSION}_*.deb"
echo ""
echo "Package contains the following executables:"
echo "  - acr (main application)"
echo "  - acr_dumpcache (cache inspection tool)"
echo "  - acr_lms_client (Logitech Media Server client)"
echo "  - acr_send_update (player state update tool)"
echo ""
echo "===== Build completed successfully ====="
