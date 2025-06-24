#!/bin/bash
cd `dirname $0`
sbuild --chroot-mode=unshare --enable-network --no-clean-source --extra-repository='deb http://deb.debian.org/debian experimental main' --build-dep-resolver=aspcud
