#!/bin/sh
# The player crate must not depend on the metadata crate, and vice versa.
#
# src/main.rs is the only place both meet. Cargo has no per-binary
# dependencies, so the metadata crate is an optional dependency behind the
# `metadata` feature, which is in `default`. That makes the honest question not
# "is it in the graph" -- with default features it is, and it should be -- but
# "is it in the graph of the library alone", which is what --no-default-features
# asks.
#
# The feature is only meaningful if both configurations are built, so this also
# builds the daemon with the feature off. That is the one thing that
# type-checks the #[cfg(not(feature = "metadata"))] branches in main.rs: the
# binary deliberately carries no `required-features`, because requiring the
# feature would mean those branches are never compiled and cannot fail.
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

# Runs `cargo tree` with the given arguments and prints its stdout on success.
# A bare `cargo tree ... 2>/dev/null | grep -q` cannot tell "no such edge" from
# "cargo tree itself failed" -- a manifest error, a lock mismatch or a
# registry problem all produce empty output piped into a grep that then
# reports the graph clean. This captures the command's own exit status so a
# failure here fails the script instead of passing every assertion vacuously.
tree_output=""
run_tree() {
  if ! tree_output=$(cargo tree "$@" --prefix none 2>&1); then
    echo "cargo tree $* failed:" >&2
    echo "$tree_output" >&2
    return 1
  fi
}

if run_tree -p audiocontrol --no-default-features --edges normal; then
  if echo "$tree_output" | grep -q '^audiocontrol-metadata '; then
    echo "the audiocontrol library depends on audiocontrol-metadata" >&2; fail=1
  fi
else
  fail=1
fi
if run_tree -p audiocontrol-metadata --edges normal; then
  if echo "$tree_output" | grep -q '^audiocontrol '; then
    echo "audiocontrol-metadata depends on audiocontrol" >&2; fail=1
  fi
else
  fail=1
fi
# Crates the player package must not *declare*. moka and aes-gcm are checked
# against the whole graph, since nothing else pulls them in and a future
# shared crate that started depending on either must still be caught. regex
# is the one exception: env_logger, which the daemon and every tool need,
# pulls it in through env_filter, so it is checked only at depth 1 -- the
# rule this enforces is about what the manifest asks for, which is the thing
# a change can get wrong.
for forbidden in aes-gcm moka; do
  if run_tree -p audiocontrol --no-default-features --edges normal; then
    if echo "$tree_output" | grep -q "^$forbidden "; then
      echo "audiocontrol depends on $forbidden, which belongs to the metadata daemon" >&2; fail=1
    fi
  else
    fail=1
  fi
done
if run_tree -p audiocontrol --no-default-features --edges normal --depth 1; then
  if echo "$tree_output" | grep -q '^regex '; then
    echo "audiocontrol depends on regex, which belongs to the metadata daemon" >&2; fail=1
  fi
else
  fail=1
fi
for forbidden in dbus alsa evdev mpd lofty; do
  if run_tree -p audiocontrol-metadata --edges normal; then
    if echo "$tree_output" | grep -q "^$forbidden "; then
      echo "audiocontrol-metadata links $forbidden, which belongs to the player daemon" >&2; fail=1
    fi
  else
    fail=1
  fi
done
# The player-only daemon still builds. This is a compile, not a graph query, so
# it is the slow half of the script -- and the only half that would catch a
# metadata call added to main.rs outside a cfg, or an `else` branch that stopped
# compiling. It rehearses the binary Phase 2 needs.
if ! cargo build -p audiocontrol --no-default-features --bin audiocontrol >/dev/null 2>&1; then
  echo "the audiocontrol binary does not build without the metadata feature; re-run" >&2
  echo "  cargo build -p audiocontrol --no-default-features --bin audiocontrol" >&2
  echo "to see why" >&2
  fail=1
fi
exit $fail
