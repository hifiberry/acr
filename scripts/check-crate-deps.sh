#!/bin/sh
# The player crate must not depend on the metadata crate, and vice versa.
#
# src/main.rs is the only place both meet. Cargo has no per-binary
# dependencies, so the metadata crate is an optional dependency behind the
# `metadata` feature, which is in `default` and required by the `audiocontrol`
# binary. That makes the honest question not "is it in the graph" -- with
# default features it is, and it should be -- but "is it in the graph of the
# library alone", which is what --no-default-features asks.
#
# `cargo tree -p audiocontrol --no-default-features -i audiocontrol-metadata`
# is the direct way to ask by hand who pulls the edge in. This script uses the
# plain listing instead, so that a package which is absent from the graph is an
# empty result rather than an error.
#
# What this deliberately does not do is look for a symbol in the built rlib.
# The metadata crate's compiled-in secrets are `pub const`, so their values are
# inlined at every use site and their names appear only as metadata in the
# crate that declares them: `strings ... | grep _OBF` over the player library
# reads zero whether the dependency is present or absent, which makes it
# evidence of nothing.
set -eu
fail=0
if cargo tree -p audiocontrol --no-default-features --edges normal --prefix none 2>/dev/null | grep -q '^audiocontrol-metadata '; then
  echo "the audiocontrol library depends on audiocontrol-metadata" >&2; fail=1
fi
if cargo tree -p audiocontrol-metadata --edges normal --prefix none 2>/dev/null | grep -q '^audiocontrol '; then
  echo "audiocontrol-metadata depends on audiocontrol" >&2; fail=1
fi
# Crates the player package must not *declare*. --depth 1 rather than the whole
# graph because one of them cannot be kept out of it: env_logger, which the
# daemon and every tool need, pulls regex in through env_filter. The rule this
# enforces is about what the manifest asks for, which is the thing a change can
# get wrong; moka and aes-gcm reach the player library no other way.
for forbidden in aes-gcm moka regex; do
  if cargo tree -p audiocontrol --no-default-features --edges normal --depth 1 --prefix none 2>/dev/null | grep -q "^$forbidden "; then
    echo "audiocontrol depends on $forbidden, which belongs to the metadata daemon" >&2; fail=1
  fi
done
for forbidden in dbus alsa evdev mpd lofty; do
  if cargo tree -p audiocontrol-metadata --edges normal --prefix none 2>/dev/null | grep -q "^$forbidden "; then
    echo "audiocontrol-metadata links $forbidden, which belongs to the player daemon" >&2; fail=1
  fi
done
exit $fail
