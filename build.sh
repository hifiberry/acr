#!/bin/bash

cd `dirname $0`

# Enable cross-compile support if configured
_CC_ENV="$(dirname "$0")/../../../scripts/cross-compile-env.sh"
if [ -f "$_CC_ENV" ]; then source "$_CC_ENV"; else echo "Not using cross-compilation (${_CC_ENV} does not exist)"; fi

# Check if DIST is set by environment variable
if [ -n "$DIST" ]; then
    echo "Using distribution from DIST environment variable: $DIST"
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
       --enable-network \
       --no-clean-source \
       $DIST_ARG
