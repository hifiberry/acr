# One owner for the current song, and who may improve it

**Date:** 2026-09-02
**Status:** Implemented
**Affects:** `src/players/player_controller.rs`, seven player backends,
`src/audiocontrol/`, the cover art provider framework, the WebSocket event
contract, `/api/now-playing`

## Problem

Two facts about this daemon are true at once, and together they mean cover art
enrichment cannot reach the clients that ask for it over REST.

**Every backend owns its own current song.** Seven files declare a
`current_song` field — `bluetooth.rs`, `generic_controller.rs`, `librespot.rs`,
`mpd.rs`, `mpris/mod.rs`, `raat.rs` and `shairport.rs` — 92 references between
them. There is no central store; `AudioController::get_song`
delegates to whichever controller is active. The storage types disagree
(`shairport` uses `Mutex`, `raat` and `librespot` use `RwLock`), and the
change-detection block is copy-pasted, with small variations, five times:

```
src/players/generic/generic_controller.rs:280
src/players/librespot/librespot.rs:666
src/players/mpd/mpd.rs:1151
src/players/mpris/mod.rs:274
src/players/raat/raat.rs:209
```

None of those copies has a test.

The LMS controller is the exception and stays one: it stores no song at all and
answers `get_song()` by querying the server (`lmsaudio.rs:668`). It keeps its
own implementation, which also means enrichment cannot reach an LMS song — a
limitation this change does not remove.

**Nothing can write into that song from outside.** The one piece of code that
enriches a playing song — the Last.fm action plugin — merges into its own copy
and publishes a `SongInformationUpdate` on the event bus. Only `api/events.rs`
(WebSocket forwarding) and `event_logger` subscribe. The player's stored song
never changes.

So `GET /api/now-playing`, which reads `player.get_song()`
(`src/api/players.rs:466`), reports the un-enriched song forever, while a
WebSocket client sees the better one. A client that reconnects and re-fetches
now-playing regresses to the worse image.

### The cover art framework is not connected to any of this

`CoverartProvider` and its manager exist only behind `/api/coverart/*`. The
manager has exactly one caller outside those routes — `artist_store.rs:334`,
for artist images. `get_song_coverart` is reached only from the route handler
at `api/server.rs:144`.

The result is that the subsystem which *has* providers cannot write, and the
code that *does* write is the scrobbler, which is not a provider and calls
Last.fm for its own reasons. Cover art lookup lives in the scrobbling plugin by
accident of history, not by design.

### Evidence

From a device playing ByteFM through MPD on 2026-09-02, before the 0.16.0
change:

- MPD reported `Title: Radical Friendship Theory - Listen To The News`; acr
  split it correctly.
- `cover_art_url` was the station favicon, 96×96.
- The daemon had already fetched the track's real cover art seconds earlier —
  the Last.fm plugin logs `Attempting to get track info for 'Listen To The
  News' by 'Radical Friendship Theory'` — and Last.fm holds a 1200×1200 cover
  for that track. It was discarded.

0.16.0 fixed the discarding. It did not fix the reach: the better image is
published on the WebSocket and `/api/now-playing` still answers with the
station logo, which `doc/api.md` and `doc/websocket.md` now say explicitly
because it could not be made otherwise without the change described here.

## What changes

### The song moves into `BasePlayerController`

`PlayerState`, which the base already owns, gains the current song. It is not
serialized into any API response — the API builds its own `PlayerInfo` — so
this changes no payload. The base already serves `position` and `last_seen`
from the same place, so the song follows an established pattern rather than
introducing a second state container.

The base gains four methods:

| Method | Purpose |
|---|---|
| `set_song(Option<Song>) -> bool` | change-detect on identity, store and `notify_song_changed` only when it changed |
| `replace_song(Option<Song>) -> bool` | store and `notify_song_changed` unconditionally; returns whether identity changed |
| `update_song(impl FnOnce(&mut Song) -> bool) -> bool` | a player revising its *own* current song in place, under one write lock; the closure reports whether it changed anything and only then are listeners notified, and the return value says whether there was a song at all. The closure runs under the write lock, which `parking_lot` does not make reentrant, so it must not call back into the base |
| `apply_song_information(&Song) -> bool` | merge a partial update from an outside lookup, store, emit `SongInformationUpdate`; returns whether anything actually changed |

Identity, for `set_song` and `replace_song` alike, is the title, the artist and
the stream URL. The artist has to be in it: only the generic controller and MPD
ever assign a stream URL, so without it the rule would be the title alone on
MPRIS, Bluetooth, RAAT and Shairport, and two consecutive tracks sharing a
title would read as one song.

`update_song` and `apply_song_information` are not interchangeable.
`apply_song_information` enforces the override policy below, which protects the
player's own artwork from an outside lookup — so a player revising its own data
through it would always be refused. `update_song` is the path for that, and it
is what shairport uses to attach the artwork its cover art watcher finds.

`get_song()` stays a required trait method: each backend still implements it,
because several do more than read the stored song (librespot fills a missing
duration out of metadata, MPD enhances the stored song, LMS queries the
server). Only `apply_song_information` gained a default implementation,
delegating to the base.

`set_song` and `replace_song` differ only in whether a same-identity
observation is stored and announced. The question that decides which one a call
site wants is **not** whether the backend polls or is called back. Transport
says nothing: a callback on a pipe is not automatically an event, and a source
that streams player state on a timer is a poller wearing a callback. The
question is:

> Does this source speak **only when something changed**, or does it speak
> **continuously regardless**?

- **A continuous source uses `set_song`** — anything that re-delivers the
  current state on a timer, on every `get_song()`, or on every line of a
  stream, whether or not it changed. Each delivery is the *same* reading
  again, minus whatever a lookup has since merged in. Storing it would erase
  the enrichment this change exists to deliver, and announcing it would put
  one `song_changed` per delivery on the bus for as long as the track plays.
  Identity gating is what makes such a source free.
- **A discrete source uses `replace_song`** — anything that speaks only when
  something actually happened. A delivery is then always news, including a
  metadata-only refresh of the song already playing (cover art arriving late
  is the usual one), which has to reach clients. Both the store and the
  notification are therefore unconditional; the return value still reports
  whether identity changed, which is what a caller gates a playback position
  reset on.

#### Deciding it, per call site

Four steps, run in order at the call site in front of you. The first one that
answers, answers; each asks for a fact about the code rather than an impression
of the backend.

**1. Who fetched the data?** Did *this* call site go and get it — a read, a
poll, a rebuild on demand — or was it handed a payload by something that
decided to speak? **Self-fetched is always continuous.** Nothing about a read
is tied to a change: the read that happens to be the first one after a change
is indistinguishable from the hundred identical ones that follow it, so storing
each unconditionally means storing the same reading over and over. This is what
settles MPD, whose `get_song` re-reads the server's status, and the D-Bus
backends, which rebuild the song from properties they have just asked for. Only
when the call site was *handed* something does the question stay open.

**2. Does the delivery have a continuous shape?** Three tells, any one of which
settles it: does the payload carry a **continuously moving field** such as a
playback position; does the call site **throttle or de-duplicate** its own
notifications; does a **watchdog declare the player dead** after a fixed
silence, which only works if deliveries are expected continuously. Any of them
means continuous.

**3. Does the sender's contract say it speaks only on a change?** This is the
one positive test for `replace_song`, and it wants evidence: an event or
variant *named* for the change it reports, a channel documented as change-only,
a sender with no timer behind it. RAAT's `PlayerUpdate::SongChanged`,
librespot's `song_changed` API event and the events posted to the generic
controller all qualify. "It arrives as a callback" does not — see the RAAT
example below.

**4. Nothing settled it — use `set_song`.** The two mistakes are not the same
size, and the default belongs to the cheap one. A wrong `set_song` on a source
that was really discrete loses a metadata refresh — late cover art, usually —
until the next identity change; the song on display is still the right song,
and the next real change repairs it. A wrong `replace_song` on a source that
was really continuous erases every enrichment within one delivery interval and
puts a `song_changed` on the bus per delivery for the whole track: the artwork
a lookup found is gone, and every client is woken repeatedly for as long as the
track plays. Take the recoverable mistake, and leave a comment saying the call
site was undecided so the next reader knows the choice was a default rather
than a finding.

RAAT is the worked example, because *both* kinds live in one file:

- `update_metadata` is the metadata pipe reader's callback. Step 1 leaves it
  open — the reader hands it a parsed payload. Step 2 settles it as
  **continuous**: every line carries `seek` and a full `now_playing` object
  that `parse_line` rebuilds an entire `Song` from, the position notification
  is throttled to a 1 s delta because of that, and `start_timeout_monitor`
  declares the player `Unknown` after ten seconds without a line. All three
  tells fire. It uses `set_song`.
- `receive_update` handling `PlayerUpdate::SongChanged` is also handed its
  payload, and none of the three tells fire on it. Step 3 settles it as
  **discrete**: the variant is named for the change it reports and nothing
  re-sends it on a timer. It uses `replace_song`.

Being a callback is what these two have in common, so it cannot be what tells
them apart — which is why the procedure never asks.

The distinction is also not about what a backend's pre-refactor code happened
to do. Several continuous backends stored unconditionally before this change,
which was harmless only because nothing could write into the stored song from
outside; once enrichment can, an unconditional store on a continuous path is a
bug.

Backends delete their `current_song` field and their copy of the
change-detection block. The comparison rule becomes one rule with one test
rather than five variants with none. `get_song` stays, because a backend's own
one usually does more than return the stored song.

### Enrichment addresses a player by source

`AudioController::apply_song_information(&PlayerSource, &Song)`.

`PlayerSource` is the right address because the Last.fm plugin already holds one
for the song it looked up. The plugin swaps its private `merge_song_updates` for
this call and stops publishing the event itself; the base publishes it, after
the stored song has been updated, so REST and WebSocket cannot disagree.

### A partial update that no longer applies is dropped

`apply_song_information` requires the partial to identify the song it is for
and to agree with the song playing. It drops an update that carries neither a
`title` nor an `artist`; of the two it does carry, every one present must match
the stored song, and one it omits asserts nothing. Title alone is therefore
enough — which matters for a source that never sends an artist at all, such as
some AirPlay senders — but a title that disagrees, or an artist that disagrees,
drops the update.

A lookup is a network round trip, and on radio the track changes underneath it.
Nothing checks this today, and 0.16.0 made it matter: before, a late answer
overwrote a field nobody was looking at; now it would overwrite a visible
image with artwork for a track that has finished.

This is the rule `doc/websocket.md` already asks clients to follow — the event
carries `title` and `artist` "so a client can confirm the update still applies
to the song it is showing". The server should not be exempt from its own
contract.

### Who may override cover art, stated rather than inherited

Today a provider cannot override a song's cover art because it has no way to
reach it. Once the write path exists, that accident stops protecting anything,
and the policy has to be explicit.

Cover art carries its provenance in `song.metadata.cover_art_source`
(introduced in 0.16.0). The rule is unchanged in substance and becomes the
whole of the policy:

| Current cover art | `cover_art_source` | May a lookup replace it? |
|---|---|---|
| none | absent | yes |
| a URL-level placeholder — a station logo | `station_logo` | yes |
| supplied by the player or by a client through `add_track` | absent | **no** |
| already resolved by a lookup | the provider's name | **no** |

Artwork that belongs to the song is never replaced, whatever a provider might
have to offer, and however much better it might be. A device shows what its
player says is playing.

The second row is the only reason this machinery exists. The fourth keeps a
resolved cover from being re-resolved on every subsequent lookup.

## Scope

**In scope**

- Song ownership moves to `BasePlayerController`; seven backends converted.
- `apply_song_information` with the staleness guard, and the
  `AudioController` entry point.
- The Last.fm plugin writes through it instead of publishing directly.
- `doc/websocket.md` and `doc/api.md` updated: `/api/now-playing` now reflects
  an enrichment, which is a change clients can observe.

**Out of scope**

- Moving cover art lookup out of the scrobbling plugin into a dedicated
  enrichment stage that consults the `CoverartProvider` manager. That is the
  right end state and this change is what makes it a move rather than a
  rewrite, but it is a separate step with its own risk.
- Any change to which providers exist or what they return.
- Lyrics, genres and playcount enrichment, which take the same path and should
  follow once the path is proven with cover art.

## Tests

The point of the change is that these become testable at all. Today all of these
are duplicated across backends and untested.

- **Change detection.** Same song, changed title, changed artist, changed
  stream URL, `None` → `Some`, `Some` → `None`. One rule, one place.
- **Partial merge.** A field absent from the update leaves the stored value
  alone; a field present replaces it; metadata merges key by key.
- **Staleness.** An update whose title and artist match is applied; one whose
  title differs is dropped; one whose artist differs is dropped.
- **Override policy.** The four rows of the table above, as they already are
  for `Song::cover_art_is_replaceable`, now exercised through
  `apply_song_information`.
- **No lock held across the event-bus publish**, which `.codereview.toml`
  calls out specifically, is covered by review rather than by a test: it is a
  property of how the code is written — every writer releases the state lock
  before it publishes — and there is no seam that would let a test observe a
  lock being held during a publish without building one for the purpose.
  `mpd.rs` dropped its lock before notifying already; the shared
  implementation encodes that once so the other eight cannot get it wrong.
- **Notification volume.** A repeated observation of the same song publishes
  one `song_changed`, not one per observation, and a rebuilt observation of a
  song that a lookup has enriched does not erase the enrichment. These two are
  what a continuous source gets wrong when it reaches for `replace_song`, and
  they are asserted at the base, where no D-Bus connection is needed: the test
  subscribes to the event bus and counts the events its own player id
  published. `raat.rs` asserts the same pair against `update_metadata`, the
  one call site whose continuity is not obvious from its signature.

The conversion of each backend is checked by the existing suite plus a run on
a real device, since most of these backends cannot be exercised without one.

## Migration

MPD first: it is the backend with tests, it is the default player on most
installs, and it is the one that demonstrates the cover art case end to end.
The remaining eight follow mechanically — delete a field, replace writes with
`base.set_song(...)` or `base.replace_song(...)`, and point `get_song` at the
stored song.

The risk is concentrated in the backends that cannot be unit tested. Converting
them in one commit each keeps a bisect useful.

## Consequences for clients

`/api/now-playing` will begin to reflect an enrichment shortly after a song
change, where today it never does. A client polling it will see
`cover_art_url` change between two polls of the same song — which the
WebSocket contract already describes for `song_information_update`, but which
REST clients have never had to handle.

This is the compatibility path: the field already changes on the WebSocket, the
marker already says when an image is a placeholder, and a client that ignores
both sees a better picture arrive slightly late rather than anything breaking.
