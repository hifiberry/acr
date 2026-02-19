#!/bin/bash

cd `dirname $0`

#Defaults
ARCH="arm64"

#Help message
print_help() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --arch <arch>    Target architecture (default: arm64, supported: arm64)
  --help           Show this help message and exit
  --dist <dist>    Target distribution
EOF
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch=*)
            ARCH="${1#*=}"
            shift
            ;;
        --help)
            print_help
            exit 0
            ;;
        --dist=*)
 	    DIST="${1#*=}"
	    shift
	    ;;
        *)
            echo "Error: unknown option $1" >&2
            print_help
            exit 1
            ;;
    esac
done

if [ "$ARCH" != "arm64" ]; then
    echo "Error: architecture not supported." >&2
    exit 1
fi

# Check if DIST is set by variable
if [ -n "$DIST" ]; then
    echo "Using distribution from DIST variable: $DIST"
    DIST_ARG="--dist=$DIST"
else
    echo "No DIST environment variable set, using sbuild default"
    DIST_ARG=""
fi

if [ -f target ]; then
    echo "Removing previous build target"
    rm -f target
fi

sbuild --chroot-mode=unshare \
       --arch=$ARCH \
       --enable-network \
       --no-clean-source \
       $DIST_ARG
