# Splitting AudioControl into a player daemon and a metadata daemon

**Date:** 2026-09-04
**Status:** Proposed
**Affects:** the crate layout, `src/helpers/*`, `src/players/mpd/library.rs`,
`src/players/lms/library.rs`, `src/data/library.rs`, `src/api/*`, `main.rs`,
Debian packaging, the nginx snippet, `doc/api.md`, `doc/websocket.md`, and the
`hifiberry-librespot` start script in HiFiBerry OS

## Problem

AudioControl is two programs in one process. One half talks to players and
must be up for the device to work: nine backends, the active-player logic, the
event bus, the WebSocket, volume and inputs. The other half talks to the
internet: MusicBrainz, TheAudioDB, fanart.tv, Last.fm, Spotify, arbitrary
cover-art endpoints, and the image cache, grader and resizer that serve what
those return. By lines the second half is larger (31k against 25k), it holds
every compiled-in secret, it decodes images at 300-600 ms a piece on a Pi, and
a panic or out-of-memory kill in it takes playback control down with it.

The coupling is uneven. The engine (`AudioController`, `EventBus`, the base
controller) imports nothing from the metadata code, and enrichment of the
playing song already enters through one method with a documented policy,
`apply_song_information`, from exactly two producers. Seven of nine backends
touch no metadata code. The fusion is in the library: `LibraryInterface` in
`src/data/library.rs` declares enrichment hooks, the MPD and LMS library
objects start artist, album-genre and image-prewarm updaters after a load, and
those updaters write back into the library's own objects through shared
mutexes. The full analysis, with line counts and the client survey, is in the
working notes that produced this spec; the facts it relies on are restated
here where a decision depends on them.

## Goals

- Two daemons, each of which can be built, tested, started and upgraded
  without the other. The player daemon runs correctly with the metadata daemon
  absent, which is also what an offline device looks like.
- No change visible to the shipped clients on the day the split lands. Every
  URL the WebUI and the iOS app use today keeps working, with the same
  response shapes, through the same `/api/audiocontrol/` prefix.
- One build on the HiFiBerry OS build host. The two binaries come from one
  Cargo workspace and one Debian source package.
- Secrets leave the player daemon. Only the metadata daemon links the
  generated secrets and opens the security store.
- The interfaces between the two are ordinary HTTP and the existing
  WebSocket, so a third party can play either role.

## Non-goals

- Changing how any provider is queried, graded or cached.
- Changing the library model, the queue model or any player backend's
  protocol handling.
- Moving library browsing out of the player daemon. The library is built from
  the MPD and LMS connections the backends own, and queue operations need the
  same connection.
- Rewriting anything in another language.
- Authentication between the two daemons. Both bind to the loopback interface
  for their private routes, and nginx remains the only public entry.

## The two daemons

### `audiocontrol` (the player daemon)

Keeps: all nine player backends, `AudioController`, `EventBus`,
`ActiveMonitor`, the event logger, the WebSocket at `/api/events`,
now-playing, commands, queue, the generic push API, volume (including the
configurator client that volume control uses to detect the sound card),
inputs, the settings API, m3u parsing, version and capabilities, the library
structure (albums, artists, tracks, playlists, search, delete, refresh,
`library_version`, ETags), images the player itself produces (album covers
read from MPD or extracted from local files, AirPlay artwork from
shairport-sync, station logos) with `?size=` resizing, thumbnail prewarm and
retired-rung purge of those, MPD lyrics files and `lyrics_url`, the
stream-title splitter's state, learning and offline fallback, genre mapping
as applied to the library, the genre configuration API, the background-jobs
API for its own jobs, and the cache statistics API for its own caches.

Binds `0.0.0.0:1080` as today.

### `audiocontrol-metadata` (the metadata daemon)

Takes: the MusicBrainz, TheAudioDB and fanart.tv clients and the rate
limiter; the Last.fm client, scrobbler and love, and the Last.fm OAuth routes;
the Spotify OAuth flow, token store and Web API routes; the external cover-art
endpoints and their worker; the cover-art manager and providers; the artist
store, gallery and uploads; the downloaded-image cache and its retired-rung
purge; artist and album enrichment from providers; the MusicBrainz-backed
part of the artist-name splitter; image metadata by URL; favourites; the
security store and every compiled-in secret.

Binds `127.0.0.1:1084`. It is never reachable from the network except through
nginx.

### Shared library crates

Five pieces of code are needed by both daemons and are owned by neither:

- `acr-types`: `Song` with its `COVER_ART_SOURCE` constants and
  `cover_art_is_replaceable`, `PlayerSource`, `ArtistMeta`, `Artist`,
  `Album`, `Track`, `Identifier`, the payload types of the interfaces
  below, the pure string helpers (`sanitize.rs`, `url_encoding.rs`, the
  separator split from `artistsplitter.rs`), and the path-rewriting
  functions from `api/urlprefix.rs`. No Rocket, no I/O.
- `acr-web`: the Rocket pieces both APIs share: the `ForwardedPrefix` request
  guard, `imageresponse.rs` (image replies with ETag and 304),
  `validated.rs`, and the `/imagecache/<path..>` handler, which each daemon
  mounts over its own cache.
- `acr-images`: `imageresize.rs` (rung snapping, `@<size>` variant naming,
  resize), format sniffing, and `image_grader.rs`. Both daemons serve images
  with `?size=` and both must name variants identically.
- `acr-store`: `attributecache.rs`, `settingsdb.rs`, `imagecache.rs`,
  `imagepurge.rs` and `backgroundjobs.rs`. Each is a process-wide singleton
  over a configured path; each daemon initialises its own over its own
  directory.
- `acr-http`: `http_client.rs`, `retry.rs` and `ratelimit.rs`. The player
  daemon uses it for its loopback calls; the metadata daemon for everything.

## Ownership of stored data

Each daemon owns its own files. Nothing is shared on disk.

| Data | Today | Player daemon | Metadata daemon |
|---|---|---|---|
| Attribute cache | one SQLite file, all prefixes | `/var/lib/audiocontrol/cache/attributes.db`, prefixes `mpd.urlmeta.`, `song_splitter:`, `imagecache:` for its own images | `/var/lib/audiocontrol/metadata/attributes.db`, prefixes `artist::*`, `album::genres::`, `theaudiodb::*`, `coverart::external::`, `image_meta::`, `imagecache:` for its own images |
| Image cache | one tree | `/var/lib/audiocontrol/cache/images/{albums,shairportsync}` | `/var/lib/audiocontrol/metadata/images/external`, and the artist directories at their current paths |
| Artist images | `/var/lib/audiocontrol/cache/artists`, `/var/lib/audiocontrol/user/images` | none | unchanged paths |
| Settings DB | one SQLite file | `/var/lib/audiocontrol/db/settings.db`: the generic `/api/settings` store, favourites written by the settings-DB provider (moves, see below) | `/var/lib/audiocontrol/metadata/settings.db`: `artist.image.<name>` selections, `datastore.artist_store.*`, `favourite_song:*` |
| Security store | one encrypted file | none | unchanged path |
| Background jobs | one registry | its own: library load, MPD database update | its own: artist metadata, album genres, prewarm, purge |

The settings-DB favourite provider moves with the other favourite providers,
so `favourite_song:*` lives in the metadata daemon's settings DB. The generic
`/api/settings` route stays on the player daemon and keeps its file.

## Interfaces between the daemons

There are five. Each is specified so that either side can be replaced by
something that speaks the same HTTP.

### 1. Now-playing enrichment: metadata daemon to player daemon

**Subscription.** The metadata daemon opens
`ws://127.0.0.1:1080/api/events` and sends
`{"players": null, "event_types": ["song_changed"]}`. It reconnects with
exponential backoff capped at 30 s. On every connect it fetches
`GET /api/now-playing` once so a song that started while it was down is
enriched too. This is exactly what the Last.fm plugin and the external
cover-art worker subscribe to on the in-process bus today; the payload carries
`player_name`, `player_id` and `song`.

**Push.** Results go to a new route on the player daemon:

```
POST /api/player/<name>/song-information
Content-Type: application/json

{ "title": "...", "artist": "...",
  "cover_art_url": "...", "liked": true,
  "metadata": { "cover_art_source": "lastfm", "...": ... } }
```

The body is a `Song` serialised as today: every field optional, absent means
"not asserted". `<name>` resolves as `/api/player/<name>/update` does, by
player name or id, case-insensitively. The handler calls
`AudioController::apply_song_information`, whose policy is unchanged and is
the whole contract: the partial must carry a title or an artist; every field
it carries must equal the current song's; only `cover_art_url`, `liked` and
the `metadata` map are merged; `cover_art_url` replaces only a placeholder;
`cover_art_source` is stamped `"enrichment"` when the partial names no source.
A change publishes `song_information_update` as today.

Responses:

| Status | Body | When |
|---|---|---|
| 200 | `{"success": true, "applied": true}` | the stored song changed |
| 200 | `{"success": true, "applied": false}` | nothing changed, or the partial no longer matched the current song |
| 400 | `{"success": false, "message": ...}` | body is not a song, or carries neither title nor artist |
| 404 | `{"success": false, "message": ...}` | no such player |

A stale answer is not an error: a lookup can finish after the next track has
started, and `applied: false` is the correct outcome. The metadata daemon
logs it at debug level and does nothing else.

This route is public on port 1080 like `/update`. It gives a third party the
same standing the metadata daemon has, which is the same standing the generic
backend gives any player.

**Reconciliation is a second, opposite call this interface needs, not an
optional extra.** The subscription above is a push: the metadata daemon learns
of a change when the event bus tells it. That is not enough for the Last.fm
worker, which has always reconciled its idea of playback state against the
player periodically — a scrobble is timed from how long a track has actually
played, and a single missed `StateChanged` (a dropped connection during the
reconnect backoff, a state change that raced the WebSocket handshake) would
otherwise leave the worker's timer running against a track that is actually
paused, for as long as the track lasts. Phase 0 gives this its own seam,
`PlaybackStateSource`, asked rather than awaited, alongside the
`SongInformationSink` the push side already used. In one process this is a
direct call into `AudioController::get_playback_state`; the interface is not
one-directional, and Phase 1 has to give this side an HTTP form as well —
`GET http://127.0.0.1:1080/api/player/<name>/playback-state` or equivalent,
polled on the same period the worker already reconciles on, with the same
tolerance for a stale or missing answer that the push side has for `applied:
false`.

### 2. Library enrichment: both directions

The player daemon stops writing provider data into its `Artist` and `Album`
objects. It keeps the fields (`Artist.metadata`, `Artist.is_multi`,
`Album.genres`) because clients read them, and fills them from what the
metadata daemon sends.

**Thumbnails: a stored value always wins, but not every backend fabricates
one.** The player daemon builds `thumb_url` as
`/api/coverart/artist/<url-safe base64 name>/image`. What differs is when. On
the MPD backend, which is the primary one, this happens on every artist-list
request: if the stored value is empty, `populate_calculated_artist_fields`
fills it in regardless of whether an image actually exists, so an MPD client
always sees a URL and a 404 from it means "no image" — exactly the
"thumbnails need no call" behaviour this spec originally described, and
still accurate there. The generic and LMS library paths do not do this: they
serve `Artist.metadata.thumb_url` exactly as stored, so an artist no lookup
has found an image for serves an empty list on those backends, and the
field's presence there is the "an image exists" signal a client acts on.

In both cases a **stored** value takes precedence and passes through
verbatim; nothing rewrites or discards it before serving. `ArtistSummary`
therefore carries `thumb_url` through enrichment rather than leaving the
player daemon to reconstruct it from the artist's name, because the attribute
cache can still hold an external provider's URL written by an older release,
and that URL must survive unchanged — it is deliberately never
prefix-rewritten like a daemon-served path would be. A route that
reconstructed `/api/coverart/artist/.../image` from the name alone, instead
of reading what enrichment stored, would silently turn such an entry into a
daemon URL for an image the store may not actually hold, trading a working
external link for a 404.

**Discovery: the metadata daemon pulls.** It polls
`GET http://127.0.0.1:1080/api/library` every 30 s and, for each player with
`has_library` and `is_loaded`, `GET /api/library/<p>` for `library_version`.
When the version differs from the one it last enriched, it fetches
`/api/library/<p>/artists` and `/api/library/<p>/albums` with `If-None-Match`
and enriches what is new. A player whose backend reports no
`library_version` (LMS) is re-fetched every 30 minutes instead. The player
daemon may shorten the wait after a load by calling
`POST http://127.0.0.1:1084/api/enrich/nudge?player=<p>`, which answers 202
and starts a pull at once; a nudge that fails is ignored, since the poll
covers it.

**Delivery: the metadata daemon calls back.** Results are posted in batches
of at most 200 items:

```
POST /api/library/<p>/enrichment
Content-Type: application/json

{ "library_version": "5e2b91c0-a3f9c1d2-42",
  "artists": [ { "name": "Pink Floyd",
                 "mbid": ["83d91898-..."],
                 "is_multi": false,
                 "genres": ["progressive rock"] } ],
  "albums":  [ { "id": "12345678",
                 "genres": ["progressive rock", "art rock"] } ] }
```

The player daemon merges with the rules the in-process updaters use today:
an empty `genres` never clears an existing list; an artist's `mbid`,
`is_multi` and `genres` replace what is stored; `library_version` bumps once
per batch if anything changed. Cached genres for albums are what makes the
existing `by-genre`, `by-category`, ETag and acr-webmcp behaviour hold.

| Status | Body | When |
|---|---|---|
| 200 | `{"applied": {"artists": n, "albums": m}, "library_version": "..."}` | merged; the returned version is the one the caller should now treat as seen |
| 409 | `{"library_version": "..."}` | the batch was computed against a version that is no longer current; the caller re-pulls |
| 404 | | no such player or no library |

Because the merge bumps `library_version`, the metadata daemon records the
version returned in the 200 as "seen" so its own write does not look like a
change on the next poll.

**Detail: the player daemon asks once.** The three artist-detail routes
(`by-id`, `by-name`, `by-mbid`) call
`GET http://127.0.0.1:1084/api/artist/<url-safe base64 name>` with a 1 s
timeout and merge the returned `ArtistMeta` (biography, biography source,
thumb and banner URLs) over what they hold. A miss, timeout or connection
failure returns what the player daemon holds, which is the summary from the
last enrichment batch. The list routes never call out.

**Kick-off.** The MPD and LMS library objects no longer start updaters. The
`LibraryInterface` trait loses `update_artist_metadata` and
`update_album_metadata`; `refresh_player_library` and `update_player_library`
keep their routes and do what they do minus the kick-off. `enhance_metadata`
in the MPD player config becomes a no-op and is removed from the docs.

This is the most work of the five interfaces. A simplification is available
if it proves too much for one phase: the player daemon can re-request the
whole album and artist set from the daemon on a timer for the ten minutes
after a load. The callback is preferred because it finishes and because it
delivers late lookups (an artist with a slow provider) without a running
timer.

### 3. Resolvers: player daemon to metadata daemon

Two questions the player daemon asks synchronously today are answered by
MusicBrainz. Both keep their offline fallback and become loopback calls with
a 5 s timeout, which is what the same lookups take in-process today. When
`services.metadata` is absent from the player daemon's config, neither call
is made and the fallback applies without waiting.

**Stream-title order.** The MPD backend keeps `SongSplitManager`,
`SongTitleSplitter`, the learned and set order per station, their
persistence under `song_splitter:<id>` and the fallback of `Artist - Title`.
The lookup moves:

```
GET http://127.0.0.1:1084/api/resolve/title-order?part1=<a>&part2=<b>
→ 200 { "order": "artist_song" | "song_artist" | "unknown" | "undecided" }
```

The metadata daemon runs `detect_order` as today: two recording searches
under the MusicBrainz rate limit. A timeout or connection failure is
`unknown`, which the splitter maps to the fallback and never learns from.

**Album-artist splitting at library load.** Both library loaders call
`musicbrainz::split_artist_names` for every album's artist string to decide
whether "Simon & Garfunkel" is one artist or two. The function returns at
once for a string with no separator; for one with a separator it asks
MusicBrainz for the combined string and, failing that, for each part, and
caches the answer without expiry under `artist::split::<name>`. The pure
part, the separator check and `split_artist_with_separators`, moves to
`acr-types` and stays in the loader. The lookup moves:

```
GET http://127.0.0.1:1084/api/resolve/artist-split?name=<n>&separators=<s1>,<s2>
→ 200 { "artists": ["Simon", "Garfunkel"] }   split
→ 200 { "artists": null }                      one artist
```

The loader asks only for strings that contain a separator, memoises answers
for the lifetime of the process, and on timeout or failure uses the plain
separator split, which is what the code does today when MusicBrainz is
disabled. A library load with the metadata daemon absent therefore completes
with the same artist list a MusicBrainz-disabled install produces now. The
metadata daemon keeps the persistent cache.

### 4. Spotify transport: player daemon to metadata daemon

The librespot backend keeps sending Web API commands (`play`, `pause`,
`next`, `previous`, `seek`, `shuffle`, `repeat`). The OAuth flow, refresh and
token store move to the metadata daemon. The player daemon gets a small
`spotify_transport` module that fetches the token from
`GET http://127.0.0.1:1084/api/spotify/access_token` (text/plain, as that
route answers today), caches it for 60 s, and issues the same requests
`Spotify::send_command` issues now. A 404 from the token route means "not
linked" and the command fails as it does today when no token is stored.

The `hifiberry-librespot` start script fetches the same route from port 1080
today. It changes to port 1084 in the same HiFiBerry OS release, and
`hifiberry-audiocontrol` declares `Breaks: hifiberry-librespot (<< that
version)` the way it already does for older players.

### 5. Favourites

`song.liked` reaches now-playing through interface 1: the Last.fm worker
already computes it. The `/api/favourites/*` routes and all three providers
(Last.fm, Spotify, settings DB) move to the metadata daemon.

## Routing and client compatibility

Both clients reach everything through `/api/audiocontrol/`, both carry
path-repair allowlists for `/api/library/`, `/api/coverart/` and
`/api/lyrics/`, and both read image paths out of responses and fetch them
unmodified. None of that changes.

The metadata daemon ships an nginx snippet with these locations, each
`proxy_pass http://127.0.0.1:1084/api/<same segment>/` and
`X-Forwarded-Prefix /api/audiocontrol`:

```
/api/audiocontrol/coverart/
/api/audiocontrol/imagecache/external/
/api/audiocontrol/lastfm/
/api/audiocontrol/spotify/
/api/audiocontrol/audiodb/
/api/audiocontrol/favourites/
```

nginx picks the longest matching prefix, so these win over the player
daemon's `/api/audiocontrol/`. Everything else, including
`/api/audiocontrol/imagecache/shairportsync/`, `/library/`, `/lyrics/`,
`/genres/`, `/cache/`, `/background/`, `/settings/` and the WebSocket, stays
on port 1080.

The same snippet also mounts `/api/metadata/` to the daemon with
`X-Forwarded-Prefix /api/metadata`, for new clients. The sub-prefixes above
are the compatibility route and are removed once both shipped clients use
`/api/metadata/`; that is a separate, later change.

Paths the metadata daemon writes into responses (`thumb_url`, localized
external images, `song.cover_art_url` values it pushes through interface 1)
are its own internal paths, `/api/coverart/...` and
`/api/imagecache/external/...`. They are rewritten with the forwarded prefix
by whichever daemon serves the response, using the shared `acr-types`
rewriting, so a client behind nginx always sees `/api/audiocontrol/...`. A
value pushed through interface 1 is stored by the player daemon in internal
form and rewritten on the way out, as the rule in `doc/api.md` already
requires of senders.

The auth manifest for the metadata daemon lists the same tiers the existing
manifest gives those paths, under `match_prefix` `/api/metadata`. The
existing `/api/audiocontrol` manifest is unchanged and keeps gating the
compatibility sub-prefixes, since gating is by path and not by upstream.

## Failure behaviour

- Metadata daemon down: playback control, the WebSocket, library lists and
  album art from local files all work. Artist thumbnails and cover-art
  lookups answer 502 from nginx, which clients treat as image failures. The
  artist-detail routes answer after the 1 s timeout with no biography. Radio
  titles split with the fallback order. Spotify Connect keeps playing but the
  librespot backend cannot send Web API commands. Nothing waits longer than
  its stated timeout, and no request path on the player daemon blocks on the
  loopback port.
- Player daemon down: the metadata daemon keeps serving its own routes,
  reconnects to the WebSocket with backoff, and resumes library polling. It
  never caches a failed pull as "empty library".
- Either daemon restarted: both are `Restart=on-failure`. The metadata daemon
  is `After=audiocontrol.service` so a boot orders them, and the player
  daemon does not reference the metadata unit.

## Configuration

The player daemon keeps `/etc/audiocontrol/audiocontrol.json`. Its
`services` section loses the provider entries and gains:

```json
"metadata": { "url": "http://127.0.0.1:1084/api", "detail_timeout_ms": 1000, "resolve_timeout_ms": 5000 }
```

Omitting `services.metadata` disables every outbound call in interfaces 2, 3
and 4.

The metadata daemon reads `/etc/audiocontrol/metadata.json`:

```json
{
  "webserver": { "host": "127.0.0.1", "port": 1084 },
  "core": { "url": "http://127.0.0.1:1080/api", "library_poll_seconds": 30 },
  "datastore": { "attribute_cache": { "dbfile": "/var/lib/audiocontrol/metadata/attributes.db" },
                 "image_cache_path": "/var/lib/audiocontrol/metadata/images",
                 "user_image_path": "/var/lib/audiocontrol/user/images",
                 "artist_store": { "cache_dir": "/var/lib/audiocontrol/cache/artists" } },
  "settingsdb": { "path": "/var/lib/audiocontrol/metadata/settings.db" },
  "security_store": { "path": "/var/lib/audiocontrol/security_store.json" },
  "images": { "sizes": [100, 140, 200, 280, 400, 800], "prewarm_sizes": [140, 200, 280] },
  "musicbrainz": {}, "theaudiodb": {}, "fanarttv": {}, "lastfm": {}, "spotify": {},
  "external_coverart": {}
}
```

The provider sections keep their current keys. `images.sizes` is configured
in both files and must agree; `GET /api/capabilities` on the player daemon
reports the player daemon's list, and the metadata daemon exposes the same
shape at `/api/metadata/capabilities`.

Both files are read through the existing `config::get_service_config`, which
takes the tree as a parameter and needs no change.

## Crate layout

The root package stays `audiocontrol` with its `src/` tree, so history,
blame and every path in `doc/` survive. The root `Cargo.toml` gains a
`[workspace]` with members under `crates/`:

```
Cargo.toml                 workspace + the audiocontrol package (src/)
crates/acr-types/          Song, PlayerSource, ArtistMeta, Artist, Album, Track, Identifier, interface payload types, sanitize, url_encoding, separator split, prefix rewriting
crates/acr-web/            ForwardedPrefix guard, imageresponse, validated, the imagecache route handler
crates/acr-images/         imageresize, format sniffing, image_grader
crates/acr-store/          attributecache, settingsdb, imagecache, imagepurge, backgroundjobs
crates/acr-http/           http_client, retry, ratelimit
crates/audiocontrol-metadata/   the metadata daemon: providers, coverart, artist_store, artist/album updaters, lastfm, spotify, favourites, image_meta, security_store, secrets, its own api/ and main.rs
```

Dependency rules, enforced by `Cargo.toml` and checked in CI with
`cargo tree`:

- `audiocontrol` depends on the five shared crates. It does not depend on
  `audiocontrol-metadata`, and its dependency graph contains neither
  `aes-gcm`, `moka`, `regex` nor the generated secrets.
- `audiocontrol-metadata` depends on the five shared crates. It does not
  depend on `audiocontrol`. It contains no `PlayerController`, no `EventBus`
  and no D-Bus, ALSA, evdev, `mpd` or `lofty` linkage.
- The five shared crates depend on none of the daemon crates. `acr-web`
  depends on `acr-types`, `acr-images` and `acr-store`; `acr-store` on
  `acr-images`; the other three on nothing in the workspace.

`build.rs` and `src/secrets.rs` move into `audiocontrol-metadata`. The root
package no longer has a build script.

The CLI tools stay `[[bin]]` targets of the package whose code they use:
`audiocontrol_dump_cache`, `audiocontrol_musicbrainz_client`,
`audiocontrol_dump_store` and `audiocontrol_favourites` move to the metadata
crate; the rest stay.

## Packaging

One source package, `hifiberry-audiocontrol`, produces two binary packages:

- `hifiberry-audiocontrol`: as today minus the moved tools, minus the secrets.
  Adds `Breaks: hifiberry-librespot (<< V)`, where `V` is the
  `hifiberry-librespot` version whose start script fetches the token from
  port 1084; it is filled in when that package is bumped in HiFiBerry OS.
- `hifiberry-audiocontrol-metadata`: `Depends: hifiberry-audiocontrol (= ${binary:Version})`
  for the `audiocontrol` user and the directories. Ships
  `/usr/bin/audiocontrol-metadata`, the four tools, `/etc/audiocontrol/metadata.json`,
  `audiocontrol-metadata.service`, `hifiberry-audiocontrol-metadata.nginx`,
  `audiocontrol-metadata-auth.json`.
- `hifiberry-audiocontrol` gets `Recommends: hifiberry-audiocontrol-metadata`,
  and the `hbos-minimal` and `hbos-full` meta-packages in HiFiBerry OS list it
  explicitly.

`debian/rules` runs one `cargo build --release --workspace` and installs from
`target/release/` into two package trees. The version is one number in
`debian/changelog` and in every `Cargo.toml`.

`postinst` of the metadata package, on first configure only (`$2` empty or
older than the release that introduces it): creates
`/var/lib/audiocontrol/metadata` and `/var/lib/audiocontrol/metadata/images`
owned by `audiocontrol`; copies `/var/lib/audiocontrol/cache/attributes.db`
and `/var/lib/audiocontrol/db/settings.db` into `/var/lib/audiocontrol/metadata/`
if they exist, so artist image selections and lookup caches survive the
upgrade. Linked accounts live in the security store, whose path does not
change. The player daemon's copies keep the foreign keys; they are bounded
and inert.

The unit:

```
[Unit]
Description=HiFiBerry AudioControl metadata service
After=network-online.target audiocontrol.service
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/bin/audiocontrol-metadata -c /etc/audiocontrol/metadata.json --log-config /etc/audiocontrol/logging.json
Restart=on-failure
RestartSec=5
User=audiocontrol
Group=audiocontrol
TimeoutStopSec=10
```

No `ConditionPathExists=/proc/asound/cards`, no audio groups, no runtime
directory.

## Documentation changes shipped with the split

- `doc/api.md`: the new `song-information` and `enrichment` routes; which
  routes now answer from the metadata daemon; the loopback routes
  (`/api/artist/<b64>`, `/api/resolve/title-order`); that `artist.metadata`
  can be empty when the metadata daemon is unavailable.
- `doc/websocket.md`: no event changes. A note that `song_information_update`
  is now produced by a separate process and can therefore arrive after the
  player daemon restarted while the metadata daemon did not.
- `doc/architecture.md`: the two-daemon picture, the five interfaces, the
  deployment table.
- `doc/metadata.md`: rewritten to describe the metadata daemon; the
  `metadata.enrichment` configuration it currently documents does not exist
  and is dropped.
- `doc/tooling.md`: building and testing a workspace.
- HiFiBerry OS `docs/backend-apis.md`: the new service and port.

## Testing

Unit tests stay inline, as this repository requires, and move with their
code. New tests, by interface:

1. `song-information` route: 200/applied, 200/not applied on a stale title,
   400 on an empty partial, 404 on an unknown player. The subscriber in the
   metadata daemon is tested against a fake WebSocket server for reconnect
   and for seeding from `now-playing`. The playback-state read the Last.fm
   worker's periodic reconciliation depends on gets its own coverage: a
   discrepancy between the worker's own idea of the state and what the read
   returns, and the read timing out or the player being unreachable.
2. `enrichment` route: merge semantics (empty genres never clear, version
   bumps once per batch, 409 on a stale version), and the puller in the
   metadata daemon against a fake player daemon for the "seen version" rule.
   The detail routes with the daemon absent, timing out, and answering.
3. `resolve/title-order` and `resolve/artist-split`: the four order outcomes,
   the split and no-split answers, the loader with the resolver absent
   producing the plain separator split, and the per-process memo.
4. `spotify_transport`: token caching, 404 handling.
5. The integration suite under `integration_test/` gains a `metadata` suite
   that starts both daemons on test ports and exercises interfaces 1 and 2
   end to end. The existing suites keep running against the player daemon
   alone, which is the test that it needs nothing else.

Every phase ends with the full suite passing in the Linux container, and with
a manual run on a device with the large library measured in `TODO.md`.

## Phases

Each phase is a release on its own and has its own implementation plan.

**Phase 0: workspace and dependency direction, one binary.** Create the
workspace and the five shared crates. Move the metadata code into the
`audiocontrol-metadata` crate as a library the `audiocontrol` binary still
links, behind traits the binary injects at startup: a `LibraryEnricher`
that replaces the library updater kick-offs and the artist-metadata cache
reads, and a `Resolver` for the two synchronous questions. Move the two
song-change subscribers into the metadata crate. Remove the
`LibraryInterface` enrichment hooks. The acceptance check is that the
`audiocontrol` library crate does not depend on `audiocontrol-metadata`;
`src/main.rs` is the only place both meet. No runtime or packaging change.

**Phase 1: the seams as HTTP, still one process.** Add the routes of
interfaces 1, 2, 3 and 4 to the respective sides, and switch the in-process
enricher, worker, resolver and Spotify transport to speak HTTP over loopback
to the same process. Mount the metadata routes under `/api/metadata/` as well
as their current paths. Ship, and observe on a device.

**Phase 2: two processes.** Add the second `main.rs`, `metadata.json`, the
second package, the unit, the nginx snippet, the auth manifest, the postinst
copy, the librespot start-script change and the docs. Split the caches by
moving the metadata daemon's image cache and settings to its directories.
Because Phase 1 already runs over loopback, this phase is packaging and
configuration.

## Risks

- **Library enrichment is the interface most likely to need a second
  iteration.** The callback design keeps every client-visible behaviour, but
  the polling discovery and the "seen version" rule are new moving parts. The
  timer fallback named under interface 2 is the escape hatch.
- **Two image caches, two settings files.** A user who reads
  `/var/lib/audiocontrol` will find the metadata daemon's data under
  `metadata/`. The `audiocontrol_dump_cache` tool moves with the metadata
  daemon and points at that file by default.
- **The path rewrite surface.** About 200 `crate::helpers::` references
  outside `src/helpers` change in Phase 0. It is mechanical, and the compiler
  finds every miss.
- **Memory on 1 GB devices.** A second Rust daemon idles at a few tens of MB.
  Image decoding leaves the player process, which is the larger effect.
