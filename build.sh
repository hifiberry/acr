#!/bin/bash

cd `dirname $0`

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
