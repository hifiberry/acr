#!/bin/bash
set -e

echo "===== Building ACR Debian Package ====="

# Check Rust version and inform if upgrade is needed
REQUIRED_RUST_VERSION="1.70.0"
CURRENT_RUST_VERSION=$(rustc --version | cut -d ' ' -f 2)

echo "Current Rust version: $CURRENT_RUST_VERSION"
echo "Required Rust version: $REQUIRED_RUST_VERSION"

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

# Create directory structure for the package
mkdir -p debian/tmp/usr/bin
mkdir -p debian/tmp/etc/acr
mkdir -p debian/tmp/lib/systemd/system

# Make sure dependencies are installed
command -v cargo >/dev/null 2>&1 || { echo "Cargo is required but not installed. Aborting."; exit 1; }
command -v dpkg-buildpackage >/dev/null 2>&1 || { echo "dpkg-buildpackage is required but not installed. Aborting."; exit 1; }

# Update version in control file from Cargo.toml
VERSION=$(grep -m 1 '^version' Cargo.toml | cut -d'"' -f2)
sed -i "s/^Version:.*/Version: $VERSION/" debian/control

# Check if we want to skip rebuilding the binary
export SKIP_BUILD=${SKIP_BUILD:-0}
if [ "$1" = "--skip-build" ] || [ "$SKIP_BUILD" = "1" ]; then
    echo "===== Skipping build stage and reusing existing binary ====="
    export SKIP_BUILD=1
    
    # Check if the binary exists
    if [ ! -f "target/release/acr" ]; then
        echo "ERROR: Cannot skip build, binary not found at target/release/acr"
        echo "       Please run the build without --skip-build first"
        exit 1
    fi
else
    echo "===== Building binary ====="
    # Tip: You can use SKIP_BUILD=1 or --skip-build to skip this step next time
    # We'll let dpkg-buildpackage handle the actual build to avoid building twice
    # So we don't run cargo build here anymore
fi

echo "===== Preparing Debian package ====="

# Copy configuration file and systemd service file
echo "Preparing configuration and service files..."
cp hifiberryos.json debian/tmp/etc/acr/hifiberryos.json.default
cp debian/acr.service debian/tmp/lib/systemd/system/

# Create postinst script to handle configuration file
cat > debian/postinst << 'EOF'
#!/bin/sh
set -e

# Function to safely copy default config if none exists
setup_config() {
    if [ ! -f /acr/hifiberryos.json ]; then
        echo "No configuration file found, copying default..."
        cp /etc/acr/hifiberryos.json.default /acr/hifiberryos.json
        # Set proper ownership
        chown acr:acr /acr/hifiberryos.json
    else
        echo "Existing configuration file found, keeping it."
    fi
}

# Function to create user, group and directory
setup_user_and_dirs() {
    # Create acr group if it doesn't exist
    if ! getent group acr > /dev/null; then
        echo "Creating acr group..."
        groupadd --system acr
    fi
    
    # Create acr user if it doesn't exist
    if ! getent passwd acr > /dev/null; then
        echo "Creating acr user..."
        useradd --system --gid acr --shell /usr/sbin/nologin --home-dir /acr acr
    fi
    
    # Create /acr directory if it doesn't exist
    if [ ! -d /acr ]; then
        echo "Creating /acr directory..."
        mkdir -p /acr
    fi
    
    # Set ownership of /acr directory
    echo "Setting ownership of /acr directory..."
    chown -R acr:acr /acr
    chmod 755 /acr
}

case "$1" in
    configure)
        setup_user_and_dirs
        setup_config
        # Enable and start the service
        if [ -d /run/systemd/system ]; then
            systemctl daemon-reload >/dev/null 2>&1 || true
            systemctl enable acr.service >/dev/null 2>&1 || true
            systemctl restart acr.service >/dev/null 2>&1 || true
        fi
        ;;
esac

exit 0
EOF

# Make the postinst script executable
chmod +x debian/postinst

# Create the Debian package
# The environment variable SKIP_BUILD is already exported above
dpkg-buildpackage -us -uc -B

echo "===== Moving package files to out directory ====="
# Move the .deb package file to the out directory
mv ../acr_${VERSION}_*.deb out/
# If there are any additional Debian files created in the parent directory, move them too
mv ../acr_${VERSION}* out/ 2>/dev/null || true

echo "===== Cleaning up ====="
rm ../acr-dbgsym*.deb
# The package will be created in the out directory
echo "Debian package created at: out/acr_${VERSION}_*.deb"

echo "===== Build completed successfully ====="
