#!/bin/sh
# Two rules about the boundary between the two halves, checked together
# because a change that breaks one usually breaks the other.
#
#   1. The player crate must not depend on the metadata crate, and vice versa.
#   2. No route on the metadata daemon may be called by the main daemon.
#
# The first is a graph question and is answered below. The second is answered
# at the end of this script; read the block there for what it can and cannot
# see.
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
# Crates the player package must not *declare*. moka is checked against the
# whole graph, since nothing else pulls it in and a future shared crate that
# started depending on it must still be caught. regex is the one exception:
# env_logger, which the daemon and every tool need, pulls it in through
# env_filter, so it is checked only at depth 1 -- the rule this enforces is
# about what the manifest asks for, which is the thing a change can get wrong.
#
# aes-gcm was forbidden here while credentials belonged to the metadata
# daemon alone. The main daemon now owns the Spotify account, so it holds
# credentials of its own through acr-secrets and legitimately depends on it.
# moka and regex are unchanged: nothing in the player package needs either.
for forbidden in moka; do
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
# ---------------------------------------------------------------------------
# The one-way seam: no route on the metadata daemon may be called by this one.
#
# This is *mostly* self-enforcing rather than checked, and the check exists to
# close what is left. There is no `services.metadata` section any more, so a
# daemon that wanted to call the metadata daemon would have to invent an
# address; what follows looks for the three ways it could.
#
# What this cannot see is a call assembled across several lines -- a base URL
# built in one place and a path appended in another. Nothing catches that but
# review, which is why the address is gone rather than merely unused: a
# reviewer meeting `get_service_config(config, "metadata")` in a diff has
# something to object to.

# (a) Reading a metadata service section back into existence. `services.core`
#     is the metadata side's own configuration and is not this; only a *read*
#     of a section named `metadata` from the player sources is.
if grep -rnE 'get_service_config\([^)]*"metadata"' --include='*.rs' src/ ; then
  echo "the main daemon reads a services.metadata section; there is none, and \
nothing in this daemon addresses the metadata daemon" >&2; fail=1
fi

# (b) A hard-coded address for it. 1084 is the port Phase 2 gives the metadata
#     daemon; 1080 is the port both halves share until then, and the default
#     `services.metadata.url` used to name. A client built against either is
#     this daemon calling that one.
#
#     `src/tools/` is excluded, and that is not a loophole. Those files are
#     separate binaries -- one `[[bin]]` each in Cargo.toml -- that are clients
#     *of* this daemon and default to its address on the command line. They are
#     not the daemon, and nothing they do can make the daemon call anything.
daemon_addresses='(127\.0\.0\.1|localhost):(1084|1080)'
hardcoded=$(find src -name '*.rs' -type f -not -path 'src/tools/*' -exec grep -nH -E "$daemon_addresses" {} + || true)
if [ -n "$hardcoded" ]; then
  echo "$hardcoded" >&2
  echo "the main daemon hard-codes a daemon address; the metadata daemon is \
not this daemon's to call" >&2
  fail=1
fi

# The arm above excludes src/tools/ because those files are CLI binaries the
# daemon does not link: src/lib.rs declares no `mod tools`, and src/tools/
# supplies ten of the eleven [[bin]] targets. A violation written there cannot
# make the daemon call anything. That is true today; this makes it checked,
# because the exclusion is a PATH filter and not a target one -- the day
# someone adds `pub mod tools;` to src/lib.rs it would silently start covering
# daemon code and nothing would notice.
if grep -qE '^\s*(pub )?mod tools;' src/lib.rs; then
  echo "src/lib.rs now declares mod tools, so check-crate-deps.sh's src/tools/ exclusion covers daemon code" >&2
  fail=1
fi

# (c) The routes that exist only to be called across this seam. Comment lines
#     are stripped first: these paths are named in several doc comments that
#     explain why the calls are gone, and a check that could not tell an
#     explanation from a call would be one nobody could keep passing.
#
#     Deliberately not listed: /artist/ and /coverart/. Both appear in the
#     player sources for good reasons -- the artist image route redirects to
#     /coverart/artist/<b64>/image and the artist lists put the same path in
#     thumb_url -- so forbidding the string would forbid naming the route as
#     well as calling it, and naming it is the point.
metadata_only_routes='/resolve/title-order|/resolve/artist-split|/enrich/nudge'
seam_calls=$(find src -name '*.rs' -type f -exec grep -nH -E "$metadata_only_routes" {} + \
  | grep -vE '^[^:]*:[0-9]+: *(//|/\*|\*)' || true)
if [ -n "$seam_calls" ]; then
  echo "$seam_calls" >&2
  echo "the main daemon names a metadata-daemon-only route outside a comment; \
these routes are the metadata daemon's and this daemon may not call them" >&2
  fail=1
fi

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
