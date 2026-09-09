# Phase 2: two processes

**Date:** 2026-09-08
**Status:** Proposed
**Affects:** `crates/audiocontrol-metadata/build.rs` and `src/secrets.rs` (both
move), `crates/acr-secrets`, a new `src/bin/audiocontrol-metadata.rs`,
`debian/` (rules, install, two units, postinst), the nginx snippet, the auth
manifests, `configs/metadata.json` (new), `doc/communications.md`,
`doc/architecture.md`, `doc/api.md`

**Follows:** [2026-09-04-player-metadata-split.md](2026-09-04-player-metadata-split.md),
whose own Phase 2 section this replaces, and
[2026-09-07-one-way-seam.md](2026-09-07-one-way-seam.md), which changed enough
that the earlier sketch is wrong in four places. Those are listed under
*What the earlier plan got wrong* rather than silently corrected.

## Problem

Both halves run in one process. Every seam between them already speaks HTTP
over loopback, and since the one-way seam every connection is opened by the
metadata half — the main daemon holds no address for it at all. What remains is
to start the metadata half as its own process.

That is mostly packaging and configuration, as the earlier spec said. But it is
not *only* that, and this document exists because four things changed under it
and one hazard appears for the first time when the processes actually separate.

## The hazard that appears only when they split

**Two processes writing one credential file will lose credentials.**

`SecurityStore::save_to_file` serialises the whole in-memory map and writes it
with `File::create`, which truncates. Each process loads its own copy at
start-up and keeps it. So:

1. The main daemon refreshes a Spotify token and rewrites the file from its
   memory.
2. The metadata daemon completes a Last.fm authentication and rewrites the file
   from *its* memory, which never saw that refreshed token.
3. The refresh is gone. Nothing errors, and the next play command re-refreshes,
   so the symptom is an account that quietly re-authenticates forever — or, if
   the refresh token itself was the casualty, one that unlinks itself.

This cannot happen today: one process, one in-memory copy. It is created by the
split, and it is created by the *previous* phase's decision to share the store
between two daemons that each own different keys in it.

**Resolution: split the file, not just the crate.** Each daemon gets its own
store, holding only its own keys — `/var/lib/audiocontrol/security_store.json`
stays with the player daemon and its Spotify keys;
`/var/lib/audiocontrol/metadata/security_store.json` holds Last.fm's session
key. `acr-secrets` remains shared as *code*; what stops being shared is the
file.

That follows the rule the account move already established — a credential lives
with the thing that uses it — and it removes the concurrency question rather
than managing it. File locking and read-before-write were both considered and
rejected: they make two processes correct about a file neither needs to share,
and every future credential would have to keep getting it right.

**Migration.** On first configure of this release, `postinst` copies the
existing store to the metadata daemon's path. Both then hold every key; each
writes only its own, and the stale copies are inert. Deleting the foreign keys
is deliberately *not* done — a partial migration that removed a key and then
failed would unlink an account, and an inert entry costs nothing. The player
daemon's store keeps Last.fm's key and vice versa, exactly as the earlier spec
already accepted for the caches.

## What must move before the split can work

**The build script and the secrets module.** `build.rs` and `src/secrets.rs`
live in `audiocontrol-metadata` and generate the obfuscated constants. The main
daemon already needs two of them — the Spotify OAuth proxy URL and its secret —
and reads them through `main.rs`, which is legal only because that file is
allowed to link both crates. **A standalone player daemon would have no OAuth
credentials at all.** They move to a crate both daemons depend on; `acr-secrets`
is the natural home, since it already exists to hold what neither half owns
alone.

**The `--no-default-features` hole.** That build currently mounts the whole
Spotify OAuth surface while `SecurityStore::initialize_with_defaults` stays
behind `#[cfg(feature = "metadata")]`, so `POST /api/spotify/tokens` answers
HTTP 200 carrying an error body — a complete, working-looking login that cannot
store anything. It is not user-facing today, because that build exists only to
prove the crate boundary. It becomes user-facing the moment the player daemon
ships as a binary in its own right, and it is untied by the same move.

## What the earlier plan got wrong

Four things, all consequences of the one-way seam.

**`/api/audiocontrol/spotify/` must not route to the metadata daemon.** The
earlier nginx list sends it to port 1084. The Spotify account is in the main
daemon now — the OAuth flow, the token lifecycle, playback control and search.
Only the two providers that consume a token stayed behind, and they are not
routes. Sending that prefix to 1084 would 404 every login.

**`services.metadata` does not exist.** The earlier spec configures it in
`audiocontrol.json`. Nothing in the main daemon addresses the metadata daemon,
which is the rule the previous phase established, and the check in
`scripts/check-crate-deps.sh` fails the build if it comes back.

**`core.url` must be set explicitly, and its absence must be an error.**
`get_service_config` falls back to the top level, and `metadata.json` puts
`webserver` there with port 1084 — so a `metadata.json` that omits or misspells
`core.url` derives `http://127.0.0.1:1084/api`, the metadata daemon's *own*
port. It then subscribes to its own event stream and polls its own library,
neither of which it serves, and fails silently forever with a reminder every
five minutes. The deriving default is right in one process and wrong in two:
**the metadata daemon refuses to start without an explicit `core.url`.**

**The `/api/metadata/` mount already exists.** The previous phase added it, so
this phase routes to it rather than creating it.

## Configuration

`configs/metadata.json` as the earlier spec gives it, with two changes: `core`
carries an explicit `url` and nothing derives one, and `security_store.path`
points at the metadata daemon's own file.

`audiocontrol.json` loses nothing — it never named the metadata daemon.

## Packaging

Unchanged from the earlier spec, which is still right: one source package, one
binary package, two binaries, two units, both enabled. The daemons install,
upgrade and roll back together, which is what keeps the seam free of
compatibility promises. The unit, the `Breaks:` on the librespot package, and
the cache migration in `postinst` all stand as written there.

Two additions to `postinst`: the security-store copy above, and creating
`/var/lib/audiocontrol/metadata` before either daemon starts.

## Failure behaviour

The matrix in `doc/communications.md` already describes the split correctly,
because Phase 1 was built against it. Two rows become real rather than
hypothetical:

| Situation | What happens |
|---|---|
| Metadata daemon down or not yet started | playback, volume, library and the WebSocket all work; no enrichment, no cover art, no scrobbling |
| Main daemon down | the metadata daemon retries with backoff, warns once and reminds every five minutes; results are dropped |

The first is the point of the whole exercise and is worth asserting in a test
rather than assuming: stop the metadata unit, confirm a play command still
answers.

**Start-up order.** The unit has `After=audiocontrol.service`, but `After` is
not readiness — the metadata daemon will often start before the player daemon
binds. That is already handled: it polls `GET /api/version` for up to 30 s and
then starts anyway, letting the subscriber's reconnect logic take over. No
`Requires=`, deliberately: the player daemon failing must not stop the metadata
daemon retrying, and the metadata daemon failing must not touch playback.

## What gets worse, and is worth measuring

**Every seam call becomes a real network round trip.** On loopback in one
process these are cheap and untimed; in two they are not. The known costs:

- The library puller reads `/artists` and `/albums` whole, and materialises the
  response string, then a `serde_json::Value`, then the parsed `Vec` — three
  copies of the same body live at once. On a 20,000-album library that is a
  multi-megabyte spike on both sides of the seam, every sweep. Worth measuring
  before deciding whether it needs streaming or paging.
- A cover-art lookup asks for a Spotify token first if none is cached. The 60 s
  TTL bounds it, and a negative answer is cached too, so an unlinked account
  costs one call a minute rather than one per lookup.
- Shutdown is two units, not one. Each pays its own grace period.

## Testing

- **Playback with the metadata daemon stopped**, as a Python integration case
  rather than a unit test. This is the property the phase exists to deliver and
  nothing currently asserts it against two real processes.
- **A refresh and an authentication that overlap.** Link Spotify, authenticate
  Last.fm, force a token refresh, and confirm both survive — the hazard above,
  reproduced deliberately before the fix and asserted after it.
- **A `metadata.json` with no `core.url` must fail to start**, with a message
  naming the key. Falsified by supplying one and watching it start.
- **The upgrade path**, on a device: a store written by 0.22.0 must open in both
  daemons after the split, with both accounts still linked. The one-process
  version of this was verified on hardware for 0.22.0 and is the model.
- The Python suite gets a second daemon in its fixture, which is the largest
  test-side change in the phase.

## Risks

**The credential migration is the one that hurts.** Losing a linked account is
a support incident, and this phase touches the store's path, its ownership and
its number of writers at once. The migration copies rather than moves for that
reason, and the device test above is not optional.

**Two units is a support surface.** "Is it running?" stops having one answer,
and a metadata daemon that is down looks to a user like metadata that has
stopped working rather than like a service that has stopped. Whatever reports
daemon health should report both.

**The seam's cost is unmeasured.** Everything above is reasoning about round
trips that have never crossed a real socket boundary in this system. The
measurement belongs in this phase, not after it.
