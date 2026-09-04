#!/bin/sh
# The player crate must not depend on the metadata crate, and vice versa.
# src/main.rs is the only place both meet, and a [[bin]] target's
# dependencies are the package's, so we check the library graph with
# --edges normal (no build or dev edges) and --no-dev-dependencies.
set -eu
fail=0
if cargo tree -p audiocontrol --edges normal --prefix none 2>/dev/null | grep -q '^audiocontrol-metadata '; then
  echo "audiocontrol depends on audiocontrol-metadata" >&2; fail=1
fi
if cargo tree -p audiocontrol-metadata --edges normal --prefix none 2>/dev/null | grep -q '^audiocontrol '; then
  echo "audiocontrol-metadata depends on audiocontrol" >&2; fail=1
fi
for forbidden in aes-gcm moka regex; do
  if cargo tree -p audiocontrol --edges normal --prefix none 2>/dev/null | grep -q "^$forbidden "; then
    echo "audiocontrol links $forbidden, which belongs to the metadata daemon" >&2; fail=1
  fi
done
for forbidden in dbus alsa evdev mpd lofty; do
  if cargo tree -p audiocontrol-metadata --edges normal --prefix none 2>/dev/null | grep -q "^$forbidden "; then
    echo "audiocontrol-metadata links $forbidden, which belongs to the player daemon" >&2; fail=1
  fi
done
exit $fail
