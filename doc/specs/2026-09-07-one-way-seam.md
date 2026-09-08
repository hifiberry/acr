# The one-way seam: making the metadata daemon a pure client

**Date:** 2026-09-07
**Status:** Implemented. Two sections were decided differently from what is
written below, and both are marked where they appear; see *Where this document
was wrong*.
**Affects:** `src/api/spotify.rs` (new), `src/players/librespot/*`,
`src/helpers/songtitlesplitter.rs`, `src/players/mpd/libraryloader.rs`,
`src/players/lms/libraryloader.rs`, `src/data/player_event.rs`,
`src/api/events.rs`, `crates/audiocontrol-metadata/src/api/*`,
`crates/audiocontrol-metadata/src/{spotify,security_store}.rs`,
a new `crates/acr-secrets`, `scripts/check-crate-deps.sh`,
`configs/audiocontrol.json`, `doc/api.md`, `doc/websocket.md`,
`doc/communications.md`

**Follows:** [2026-09-04-player-metadata-split.md](2026-09-04-player-metadata-split.md),
whose Phase 1 is complete. This is Phase 1.5: it runs before that document's
Phase 2, while both halves still share a process and every path below can be
changed and tested without deploying a second daemon.

## Problem

Phase 1 put every seam between the player half and the metadata half onto
HTTP. It did so symmetrically: each half serves routes the other calls. That
works, and it is more complexity than the problem needs.

Three specific costs.

**Two servers to reason about.** Nine calls cross from metadata to player and
five from player to metadata. Because both directions exist, both halves must
listen, both need a configured address for the other, and every new seam
requires deciding which side serves it — a decision with no obvious rule, which
is how `capabilities` ended up mounted in a place that stopped the daemon
starting.

**Two discovery mechanisms doing one job.** Now-playing enrichment discovers
work by subscription; library enrichment discovers it by polling every 30 s and
by an advisory nudge. Both then post results back the same way. The polling
half carries the `SeenVersions` bookkeeping — fetch a version, compare it
against a recorded one, get the prefixing right on both sides — which produced
the one serious defect of Phase 1 and was invisible to a test suite whose
fixtures used the same literal on both sides of the comparison.

**Playback control depends on the metadata half.** The librespot backend
translates `PlayerCommand::Play`, `Pause`, `Next` and the rest into Spotify Web
API calls, and fetches the bearer token for them across the seam
(`src/players/librespot/librespot.rs`, nine call sites). If the metadata half
is down, pressing play does nothing. That inverts the priority the previous
spec set out, where the player half must be up for the device to work and the
metadata half is the part that may be absent.

Two calls also block a latency-sensitive path. `split_album_artist` runs once
per album during a library load, and `title_order` runs on every stream title
change; both wait on the network with a 5 s bound.

## Goals

- **One server in the seam.** Every connection between the two halves is opened
  by the metadata daemon. The main daemon never addresses it.
- **Playback control survives the metadata daemon being absent**, including at
  boot.
- No blocking network call on a library load or a stream title change.
- Fewer moving parts in the staleness bookkeeping, which is where the defects
  have been.
- No change to what a client sees, beyond one additive event.

## Non-goals

- Splitting the processes. That remains Phase 2 and is unchanged by this.
- Moving Last.fm. See *What does not move*.
- Removing the metadata daemon's server. It keeps one for clients; what it
  loses is being called by the main daemon.
- Changing the merge policy in `apply_song_information`, the enrichment 409, or
  either library token.

## The rule

> **No route on the metadata daemon may be called by the main daemon.**

Everything below follows from it. The rule is worth stating as a rule rather
than as a list of calls, because the list will change and the rule should not.

After this change the seam has four connections, all opened by the metadata
daemon, and **no exceptions**:

| | Connection | Carries | Kind |
|---|---|---|---|
| ① | `ws://…/api/events` | song, state and **library** changes, downward | push, one long-lived socket |
| ② | `GET /api/library`, `/library/<p>`, `/artists`, `/albums` | lists to enrich | read |
| ③ | `POST /api/library/<p>/enrichment`, `POST /api/player/<n>/song-information` | results, upward | write |
| ④ | `GET /api/spotify/access_token` | a bearer token for Spotify search | read, cached 60 s |

Data still travels both ways. What is one-way is *who opens the connection*,
and that is the property that removes a server.

## What moves: Spotify

The Spotify account is an account for a player backend. Its primary consumer is
playback control, in the main daemon, at nine call sites; its secondary
consumers are two metadata providers. It moves to the main daemon.

**To the main daemon** — the OAuth flow, the token lifecycle, and playback:

| Route | Why |
|---|---|
| `POST /tokens`, `GET /status`, `POST /logout` | token lifecycle |
| `GET /oauth_config`, `/create_session`, `/login/<id>`, `/poll/<id>`, `/check_server` | the browser-mediated OAuth flow |
| `GET /access_token` | now serves the metadata daemon rather than calling it |
| `GET /playback`, `POST /command/<c>`, `GET /currently_playing` | playback control and state |

The refresh loop moves with them, and the Spotify credentials move into the
main daemon's security store. **This is the point of the move**: after it,
playback control needs nothing from the metadata daemon.

**Stays in the metadata daemon** — the parts that only consume a token:

- `POST /search`, which is a lookup rather than control
- `SpotifyCoverartProvider`, which searches Spotify for artist, album and track
  images
- `SpotifyFavouriteProvider`

All three obtain a token through connection ④ and degrade to contributing
nothing when there is none. That is the correct failure mode for metadata.

## What does not move: Last.fm

Last.fm is also a login, and the same reasoning gives the opposite answer: its
consumer is the scrobbler, which reacts to now-playing and lives on the
metadata side. The rule this change follows is that **a credential lives with
the thing that uses it**, so Last.fm's session key stays where the scrobbler
is.

The consequence is that both daemons hold credentials, which is what forces the
security store to be shared.

## Ownership of stored data

`SecurityStore` currently lives in the metadata crate and holds the Spotify
tokens and the Last.fm session key together, encrypted with a build-time
obfuscated key. After this change both daemons need it, so it moves to a new
shared crate, `crates/acr-secrets`, with no behaviour change: same file format,
same key derivation, same API.

Each daemon reads and writes its own keys in the one store file. They do not
share a key namespace beyond the file, and neither reads the other's entries.

**This changes the dependency rule.** `scripts/check-crate-deps.sh` currently
fails the build if the player package depends on `aes-gcm`, on the grounds that
credentials belong to the metadata daemon. That premise no longer holds: the
main daemon now owns the Spotify credentials, so `aes-gcm` must be permitted
for it. `moka` and `regex` stay forbidden — nothing here changes their case.
The check must be edited deliberately, with the reason recorded next to it, and
not simply relaxed until it passes.

## What replaces each removed call

### `POST /enrich/nudge` → a `library_changed` event

The event bus already carries eleven event types over `/api/events`. One more
announces that a player's library has finished loading or reloading, naming the
player and its current `library_version` and `library_generation`.

The metadata daemon reacts to it instead of polling for the same fact. On every
connect it also asks once for the current versions, the same way the
now-playing subscriber already seeds itself — a reconnect means events were
missed, and the seed is what recovers them.

This event is client-visible and additive. A WebUI could use it to refresh a
library view; documenting it is required either way.

### `GET /api/library` polled every 30 s → the event, with the poll demoted

The poll is **demoted, not deleted.** Polling heals everything: a missed nudge
costs 30 s today, while a missed event could cost until the next library
change. The event becomes the primary mechanism and the poll drops to a slow
backstop — 10 minutes rather than 30 seconds — which keeps the recovery
property and still removes the machinery from the common path.

`SeenVersions` shrinks accordingly. It still records what has been enriched,
because a batch's 200 returns a moved version, but it stops being the thing
that *discovers* work.

### `GET /api/player` polled every 30 s → the existing subscription

The subscription already carries `state_changed`, and events are not lost on an
intact connection; a reconnect is covered by the per-connect seed. Rather than
delete this outright on that reasoning, **lengthen it to 5 minutes first and
observe on a device.** If nothing regresses over a release, remove it and with
it the `PlaybackStateSource` trait.

### `GET /resolve/title-order` → decide locally, correct afterwards

**This section was implemented differently from what follows, and what follows
is left in place because the reasoning against it is the substance of the
change.** The correction does *not* travel through `POST song-information`.
That route identifies a song by its title and artist and refuses a partial
disagreeing with either -- which an order swap does by construction, since it
carries the artist as the title and the title as the artist. The merge policy
exists to reject information for a song that is no longer playing, and a swap
is indistinguishable from that.

What was built instead: the correction is a per-station observation, posted to
`POST /player/<name>/splitter/<station>/observation`, feeding the learned
statistics and never the forced order. Two consequences follow, and neither is
what the paragraph below implies. **The currently playing track keeps the split
it was given**, right or wrong -- correcting it would need the merge policy
changed, which is an explicit non-goal. And learning is not immediate:
`check_and_set_default_order` needs twenty decided observations at ninety-five
per cent agreement, so a station announcing "Title - Artist" reads the wrong
way round for roughly an hour of continuous listening. Setting the order
outright for that station takes effect at once and beats anything learned.

`SongTitleSplitter` already prefers a locally held answer: an explicitly
configured order (`forced_order`), then an order learned from statistics
(`default_order`), and only then a lookup. Removing the lookup leaves the first
two, plus a heuristic for the unseen case.

The metadata daemon corrects a wrong guess through `POST song-information`,
which already exists and already has a merge policy — the same way cover art
arrives late and updates the song. The correction also feeds `update_stats`, so
the station still learns its order; it learns from corrections instead of from
lookups.

### `GET /resolve/artist-split` → the enrichment batch

`split_album_artist` runs in the MPD and LMS library loaders, once per album.
Those album artists are known in advance, so the correct split can travel in
the enrichment batch that already carries `is_multi`, `mbid` and genres for the
same artists.

`ArtistSummary` gains an optional field naming the artists a name splits into.
Both loaders already apply `summary.is_multi` to a stored artist
(`src/players/mpd/library.rs:632`, `src/players/lms/library.rs:204`), so the
delivery path exists; what is new is acting on the split.

**This is the one real behaviour change in this document, and it is a
regression on first load.** Today the split is correct immediately, at the cost
of a blocking call per album. Afterwards, the first load that meets a new album
artist uses the plain separator split, and the correction arrives with the
enrichment sweep — so an album may briefly show two artists where there is one,
or the reverse.

That is the same eventual consistency genres, images and biographies already
have on that screen, and it buys the removal of a per-album network call from a
load that can cover 200,000 songs. It is stated here rather than discovered
later.

### `GET /artist/<b64>` → the enrichment batch; `GET /coverart/artist/<b64>/image` → a redirect

**This section was implemented differently from what follows, and what follows
is left in place because the reasoning against it is the substance of the
change.** These two were the last live calls, and they are not proxying: the
main daemon does not return the metadata daemon's answer, it merges each into a
differently shaped answer of its own. `artist_detail` supplies the biography,
its source and the banner to `GET /api/library/<p>/artist/by-*`, and
`artist_image` supplies bytes to `GET /api/library/<p>/image/artist:<name>`.

What was built instead:

- **Artist detail travels in the enrichment batch.** `ArtistSummary` gains
  `biography`, `biography_source` and `banner_url` beside the `thumb_url` it
  already carried, and the library's merge writes them. The route serves what
  it holds. A client sees the same fields in the same shape, arriving once the
  sweep has reported rather than immediately — the eventual consistency the
  genres and thumbnails in that same response already had. The cost is memory:
  a biography is the largest thing held per artist, and it is now held on the
  player side.
- **The artist image route names its destination.** Bytes cannot travel in a
  batch and the player half has nothing local to answer from, so the route
  answers `302` to `/coverart/artist/<b64>/image` — prefix-rewritten, with
  `size` carried across. That path is the one the artist lists have always put
  in `thumb_url`, so a client following the redirect lands where the list would
  have sent it. A client that follows no redirects sees a 302 where it saw
  bytes; that is the one client-visible change in this document beyond the
  additive event, and it is in the changelog.

The original plan — route `/api/metadata/` in nginx, let clients fetch both
themselves, and keep the two calls as documented exceptions until the WebUI and
`hbos-ios` migrate — was rejected for two reasons. It does not remove the calls,
so `services.metadata` has to stay, and with it the address that makes every
other violation cheap; the rule would then hold by convention on exactly the
path that most invites breaking it. And it is a *larger* change for a client
than either of the above, not a smaller one: a client would have to make a
second request per artist and merge two responses, where the redirect costs it
nothing and the batch costs it nothing at all.

## Configuration

`services.metadata` disappears. Nothing in the main daemon addresses the
metadata daemon, so it needs no address for it, and the configuration hazard
recorded in `doc/communications.md` goes with it — a `metadata.json` that omits
`core.url` can no longer derive the metadata daemon's own port and subscribe to
itself.

`services.core` is unchanged: the metadata daemon still needs the main daemon's
address, and an absent section still means the documented defaults rather than
silence.

## Failure behaviour

| Situation | Before | After |
|---|---|---|
| Metadata daemon down at boot | playback control dead — no token | **playback works**; no enrichment, no cover art |
| Metadata daemon down later | as above | as above |
| Main daemon down | metadata retries; results dropped | unchanged |
| Library loads while metadata is down | nudge fails, poll catches it within 30 s | event missed, backstop poll catches it within 10 min |
| Stream title changes | 5 s network call, then a decision | decision immediately; a wrong one stands for that track, and the station is learned after ~20 observations |
| Library load, new album artist | correct split, 5 s per album | plain split, corrected by the sweep |
| No Spotify account linked | playback commands fail; no Spotify cover art | unchanged |

The first row is the reason for the change.

## Client compatibility

One additive event, `library_changed`, documented in `doc/websocket.md` with
the version it appears in and the note that a client which does not subscribe
to it sees no difference. No existing event changes shape and no path moves.

One route changes its response, which this section originally said would not
happen: `GET /api/library/<p>/image/artist:<name>` answers `302` to a path the
artist lists already hand out, instead of the bytes. Every client that follows
redirects is unaffected; one that follows none is not. It is in the changelog,
with "follow redirects" said plainly.

The Spotify routes move between *processes*, not between *paths*: nginx routes
`/api/audiocontrol/spotify/…` to the main daemon after this change rather than
the metadata daemon, and clients see the same URLs. The nginx snippet is
updated in the same change.

## Crate layout

One new crate, `crates/acr-secrets`, holding the security store. Everything
else stays. The metadata crate keeps its Spotify search and provider code and
loses the account code.

## Testing

- The rule itself: a check that the main daemon's sources build no HTTP client
  addressed at the metadata daemon. Once `services.metadata` is gone there is
  nothing to build one from, which makes this mostly self-enforcing; the check
  closes the rest and belongs beside `check-crate-deps.sh`.
- The `library_changed` event: a test that a library load emits it, and that
  the metadata side reacts to it — falsified by removing the emission and
  confirming the reaction stops.
- The demoted poll: a test that a *missed* event is still recovered by the
  backstop, which is the property the demotion must not lose.
- The split correction: a test that a plain-split library is corrected by a
  batch naming the true split, in both directions — a name wrongly split, and a
  name wrongly kept whole.
- Playback with the metadata daemon absent: a test that a play command succeeds
  with no metadata side configured at all. This is the goal of the Spotify move
  and nothing else asserts it.
- The security store: the existing tests move with it unchanged. A store
  written by the current release must still be readable, and that needs a
  fixture rather than a round-trip test.

Every test runs twice, the second time unprivileged, as in the previous phase.

## Risks

**The split regression on first load** is the one users could notice. It is
bounded — one sweep — and consistent with the rest of that screen, but it is a
visible change and should be called out in the changelog rather than shipped
quietly.

**Removing the state poll on reasoning rather than evidence.** The plan
lengthens it and observes rather than deleting it, precisely because the
argument for deleting it is a claim about TCP and not a measurement.

**The security store move touches credentials.** A migration that loses a
user's Spotify or Last.fm login is a support incident. The store format does
not change, so the risk is in the move rather than the format, and the fixture
test above is what covers it.

**The dependency-rule edit could be over-broad.** Permitting `aes-gcm` for the
main daemon is correct; permitting it by weakening the check to silence a
failure would not be. The edit names the crate and the reason.

## Where this document was wrong

Recorded rather than quietly corrected, because both mistakes are the kind that
would recur.

**It contradicted itself about the two artist calls.** *What replaces each
removed call* deferred them to client migration and said "the rule has two
documented exceptions", while *Configuration* said `services.metadata`
disappears because "nothing in the main daemon addresses the metadata daemon".
Both cannot hold: the exceptions are calls, and a call needs an address. The
contradiction was resolved in favour of the rule, and the section above says
how.

**It called the two calls proxying, and they are not.** A proxy returns the
other side's answer; these merge it into a differently shaped one. That
mischaracterisation is what made "route it in nginx" look like a complete
answer, when in fact it moves work onto every client. Naming what a call
actually does, rather than what it resembles, is what changed the decision.
