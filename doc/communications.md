# How the parts communicate

AudioControl is two processes with a seam between them. On one side is the
*player* half: player backends, the library, the event bus, the REST and
WebSocket API — the `audiocontrol` daemon, on port 1080. On the other is the
*metadata* half: MusicBrainz, TheAudioDB, FanArt.tv, Last.fm, cover art and the
artist store — the `audiocontrol-metadata` daemon, on port 1084. Neither is
linked into the other. Everything that crosses between them is an HTTP request
to `127.0.0.1:1080`, the port the player daemon listens on.

**The seam runs one way.** Every connection across it is opened by the metadata
half. The player half holds no address for the metadata half and no client for
it: there is no `services.metadata` section, and a violation would have to
hard-code an address, which `scripts/check-crate-deps.sh` looks for. Data still
travels both ways -- what is one-way is *who calls whom*, and that is the
property that means only one of the two needs to be listening for the other.

This document is the map of that seam: what the parts are, what each owns, and
exactly how each pair talks. It is written from the code. Where the design
document ([`specs/2026-09-04-player-metadata-split.md`](specs/2026-09-04-player-metadata-split.md))
and the code differ, the code is described here and the difference is called
out — that has happened five times, and each time the code had found the
better answer.

For the shorter version, see [architecture.md](architecture.md#the-two-halves-talk-over-loopback-one-way).
This file is the detail behind it.

---

## Contents

- [Why it is built this way](#why-it-is-built-this-way)
- [The parts](#the-parts)
- [The dependency rule, and how it is enforced](#the-dependency-rule-and-how-it-is-enforced)
- [The seams at a glance](#the-seams-at-a-glance)
- [Seam 1: now-playing enrichment](#seam-1-now-playing-enrichment)
- [Seam 2: library enrichment](#seam-2-library-enrichment)
- [Seam 3: the resolvers](#seam-3-the-resolvers--gone)
- [Seam 4: the Spotify access token](#seam-4-the-spotify-access-token)
- [Seam 5: favourites](#seam-5-favourites)
- [What the player half serves without asking](#what-the-player-half-serves-without-asking)
- [The two library tokens](#the-two-library-tokens)
- [Start-up and shutdown](#start-up-and-shutdown)
- [What clients see](#what-clients-see)
- [When something is missing or slow](#when-something-is-missing-or-slow)
- [What the split changed, and what it cost](#what-the-split-changed-and-what-it-cost)
- [Where this map is thin](#where-this-map-is-thin)

---

## Why it is built this way

The two halves have almost nothing in common. The player half is driven by
events from hardware and local daemons; it must answer a keypress in
milliseconds and must never block on the network. The metadata half is almost
entirely network: rate-limited third-party APIs, retries, caches, image
downloads, OAuth tokens. Running them in one address space meant one crash, one
memory profile, one restart, and one set of credentials in a process that also
holds a socket open to the local network.

They are two processes now. Doing it in one step would have meant inventing
every interface and moving every file at once, with no working state in
between. So the seams were converted to HTTP *first*, while both halves still
shared a process. By the time the processes separated, every call already
crossed a network boundary with a timeout, a serialised payload and a
documented failure path — against the same routes the metadata daemon now calls
for real. The move itself was a packaging and configuration change rather than a
redesign, which is what the staging bought.

**The split is not a deployment choice a device makes.** Two binaries in one
package, two systemd units, both enabled; they install, upgrade and roll back
together. There is no installation that runs "just the player half", and no
device runs one half at a different version from the other. That is what keeps
the seam free of compatibility promises: it is an internal interface between two
processes of the same build, not an API with versions on either side of it.

---

## The parts

```mermaid
graph TB
    subgraph player["audiocontrol — the player daemon, port 1080 — src/"]
        API["api/<br/>REST + WebSocket<br/>Rocket, port 1080"]
        PC["players/<br/>MPD · MPRIS · Spotify<br/>RAAT · Shairport · Bluetooth"]
        LIB["data/<br/>library, songs, albums<br/>library_version + generation"]
        BUS["audiocontrol/eventbus<br/>typed event bus"]
    end
    subgraph meta["audiocontrol-metadata — the metadata daemon, port 1084 — crates/audiocontrol-metadata/"]
        PROV["providers<br/>MusicBrainz · TheAudioDB<br/>FanArt.tv · Last.fm · Spotify"]
        STORE["artist_store<br/>cover art, images, bios"]
        WS["now_playing_ws<br/>WebSocket subscriber"]
        PULL["library_puller<br/>event-driven<br/>10 min backstop"]
        CC["core_client<br/><b>CoreClient</b>"]
        MAPI["api/<br/>artist · resolve · coverart<br/>lastfm · favourites<br/>Rocket, port 1084"]
    end

    subgraph shared["shared crates — neither half owns them"]
        T["acr-types<br/>Song, Album, Artist<br/>the seam traits"]
        H["acr-http<br/>blocking client<br/>retry, rate limit"]
        S["acr-store<br/>SQLite caches<br/>settings, jobs"]
        W["acr-web<br/>Rocket guards<br/>prefix, imagecache"]
        I["acr-images<br/>resize, sniff, grade"]
    end

    CC -.->|"HTTP → 127.0.0.1:1080"| API
    WS -.->|"WebSocket ← 127.0.0.1:1080"| API

    LIB --> T
    API --> W
    LIB --> S
    API --> I
    PROV --> T
    CC --> H
    STORE --> S
    MAPI --> W
    STORE --> I

    style player fill:none,stroke:#4a7
    style meta fill:none,stroke:#a47
    style shared fill:none,stroke:#888
```

The dotted arrows are the only paths between the halves, and both start on the
metadata side. There is no arrow the other way and no box to draw one from:
`MetadataClient` used to sit in `src/audiocontrol/` and is gone.

| Part | Owns |
|---|---|
| `src/api/` | Every client-facing route and the WebSocket. Rocket, bound to port 1080. |
| `src/players/` | One module per backend. Each turns its source's notion of "what is playing" into the shared `Song` and `PlaybackState`. |
| `src/data/` | The in-memory library, and both library tokens. |
| `src/audiocontrol/` | The event bus, and the local decisions that replaced the seam calls the player half used to make. |
| `crates/audiocontrol-metadata/` | Every third-party provider, the artist store, the cover-art pipeline, the Last.fm credentials, and `CoreClient`. Its `src/bin/audiocontrol-metadata.rs` is the second daemon: Rocket bound to port 1084, serving the metadata routes at their historical paths and again under `/api/metadata/`. |
| `crates/acr-types/` | The data types both halves exchange, and the **traits that define the seams**. No I/O. |
| `crates/acr-http/` | The blocking HTTP client both halves use, with retry and per-service rate limiting — which spaces request starts *and* bounds how many are in flight, since spacing alone let requests pile up at a slow provider. |
| `crates/acr-store/` | SQLite attribute cache, settings database, image cache, background-job registry. |
| `crates/acr-web/` | Rocket request guards and responders shared by both — the forwarded-prefix guard, the image responders, the image-cache routes. |
| `crates/acr-images/` | Resizing, variant naming, format sniffing, image grading. |

The traits in `acr-types` are the load-bearing part of this arrangement. Each
seam is a trait, and each trait has two implementations: an in-process one used
by tests, and an HTTP one used by the daemon. Neither half names the other's
type.

| Trait | Implemented over HTTP by | Lives in |
|---|---|---|
| `SongInformationSink` | `CoreClient` | metadata half |
| `PlaybackStateSource` | `CoreClient` | metadata half |
| `AccessTokenSource` | `CoreClient` | metadata half |
| `EnrichmentSink` | `HttpEnrichmentSink` | metadata half |

**Every one of them is implemented on the metadata side**, which is the same
statement as the rule: a trait implemented there is one that side calls out
through. There is no longer a trait the player half calls, and
`acr-types::enrichment` no longer defines one — `Resolver` is deleted and
`LibraryEnricher` has moved into the metadata crate, where it now describes one
part of that crate starting a sweep in another.

The three `CoreClient` traits are deliberately carried by one type. A wrapper
per trait would invite one being pointed at the wrong daemon, which is a
mistake that could still be made in this direction.

---

## The dependency rule, and how it is enforced

Two rules share one script, because a change that breaks either usually
touches the same files.

**The `audiocontrol` library must not depend on `audiocontrol-metadata`.** Not
in either direction, in fact: neither crate may depend on the other. They meet
in exactly one file, `src/main.rs`, which links the metadata crate behind a
default-on `metadata` feature.

This is not a convention. `scripts/check-crate-deps.sh` runs
`cargo tree --edges normal` in both directions and fails the build if either
edge appears. It also fails if the player package declares a crate that belongs
to the metadata side — `aes-gcm` and `moka` against the whole graph, `regex` at
depth 1 — and it builds the `audiocontrol` binary twice, once
`--no-default-features` and once `--no-default-features --features alsa`, to
prove the feature-gated blocks still compile with the metadata half absent.

Those two builds are the ones that catch real mistakes. Every
`#[cfg(feature = "metadata")]` block in `main.rs` needs a counterpart that
compiles without it, and it is easy to add the first and forget the second.

**The second of them is the build that ships.** The player daemon in the
package is built `--no-default-features --features alsa`, so what the check
proves compiles is what runs on a device — the bare `--no-default-features`
build alone did not, since `alsa` gates real code of its own and a
`not(metadata)` branch that compiles only because the alsa feature dragged
something in would have gone unnoticed. Built with the default features instead, the player
daemon carries a full in-process metadata half of its own — a second artist
store, a second settings database, a second attribute cache, and its own
answers on `/api/coverart/methods`, `/api/favourites/providers`,
`/api/lastfm/status`, `/api/metadata/capabilities` and
`/api/resolve/title-order`. Two of those instances on one device would mean two
enrichers and two scrobblers on the same events. On the shipped build those
paths 404 on 1080 and answer on 1084, which was confirmed on two devices.

**No route on the metadata daemon may be called by the main daemon.** The same
script checks this, in three greps over `src/`: a read of a `metadata` service
section, a literal `127.0.0.1:1080` or `:1084` outside `src/tools/` (which holds
separate binaries that are clients *of* this daemon), and any of
`/resolve/title-order`, `/resolve/artist-split` or `/enrich/nudge` outside a
comment. Each fails the build on its own, and each was confirmed to fail by
writing the violation.

The greps are the smaller half of the enforcement. The larger half is that
there is no address to build a client from: `services.metadata` does not exist
and nothing reads it, so a violation has to *invent* an address, which is what
the first two greps look for and what a reviewer meeting it in a diff has
something to object to. What none of this can see is a call assembled across
several lines — a base URL built in one place, a path appended in another.

Run it before every push:

```sh
sh scripts/check-crate-deps.sh
```

> **It leaves a `--no-default-features` binary in the target directory.** That
> binary has no metadata routes, which is correct on a device — the metadata
> daemon answers them — but wrong for anything that expects one process.
> Rebuild with default features, or start a metadata daemon alongside it,
> before running the daemon or the Python suite, or roughly forty route lookups
> will 404 and look exactly like a regression. This has cost three full test
> runs.

---

## The seams at a glance

```mermaid
graph LR
    P["player half"]
    M["metadata half"]

    P -->|"① events over WebSocket<br/>song, state, library"| M
    M -->|"② GET library, artists, albums"| P
    M -->|"③ POST song-information<br/>POST library enrichment"| P
    M -->|"④ GET spotify/access_token"| P
```

Four connections, and the metadata half opens all four.

| # | Connection | Transport | Timeout | On failure |
|---|---|---|---|---|
| ① | Song, state and library changes | WebSocket, `ws://…/api/events` | reconnect backoff capped at 30 s | nothing is enriched during a gap; a per-connect seed recovers the current song and asks the puller to sweep every library |
| ② | Library contents | `GET /api/library`, `/library/<p>`, `/artists`, `/albums` | 5 s | the sweep does not start; the next event or backstop sweep retries |
| ③ | Results, upward | `POST /api/player/<n>/song-information`, `POST /api/library/<p>/enrichment` | 5 s | a song result is dropped and the next lookup re-sends; a batch's 409 ends the sweep and the next pull retries; 404 ends it |
| ④ | Spotify access token | `GET /api/spotify/access_token` | 5 s | `None`; 60 s cache bounds re-asking |

Connection ① also carries the playback state poll, `GET /api/player`, which is
a fifth *route* on the same client rather than a fifth connection: the Last.fm
worker reconciles against it every five minutes as a backstop to the
subscription.

**There is one client left, not two.**

- **`CoreClient`** — `crates/audiocontrol-metadata/src/core_client.rs`, metadata
  half, pointed at `services.core.url`. Carries `SongInformationSink`,
  `PlaybackStateSource` and `AccessTokenSource`, and the library reads, on one
  5 s timeout for every call.
- **`MetadataClient`** — deleted. It was the player half's client for the
  metadata half, and its last two calls were `GET /artist/<b64>` and
  `GET /coverart/artist/<b64>/image`, made to answer a client's request on the
  player half's own artist routes. Both are gone: [what the player half serves
  without asking](#what-the-player-half-serves-without-asking) says what
  replaced them.

`services.metadata` is gone with it. That is not tidying — it is what makes the
rule hold by construction, because there is now nothing to build a client
*from*. What is left for a check to catch is a hard-coded address, and
`scripts/check-crate-deps.sh` greps for three shapes of one: a read of a
`metadata` service section, a literal `127.0.0.1:1080` or `:1084` outside
`src/tools/`, and any of `/resolve/title-order`, `/resolve/artist-split` or
`/enrich/nudge` outside a comment.

---

## Seam 1: now-playing enrichment

The player half announces what is playing; the metadata half looks it up and
sends back what it found. Three calls, in two directions.

```mermaid
sequenceDiagram
    autonumber
    participant PL as player backend
    participant BUS as event bus
    participant API as api/events (WS)
    participant WS as now_playing_ws
    participant W as cover art / Last.fm workers
    participant CC as CoreClient
    participant CTRL as PlayerController

    Note over WS,API: on connect, and after every reconnect
    WS->>API: subscribe {"players":null,<br/>"event_types":["song_changed","state_changed"]}
    WS->>CC: GET /api/now-playing  (seed)
    CC-->>WS: current song, if any

    PL->>BUS: song changed
    BUS->>API: PlayerEvent
    API-->>WS: song_changed {player, song}
    WS->>W: NowPlayingEvent

    W->>W: provider lookups (rate limited)
    W->>CC: POST /api/player/<name>/song-information<br/>{title, artist, cover_art_url, …}
    CC->>CTRL: apply_song_information(partial)
    CTRL-->>CC: applied: true / false
    CC-->>W: 200 {applied}

    Note over CTRL: on applied: true — publish<br/>song_information_update to clients

    loop every 5 min, Last.fm worker
        W->>CC: GET /api/player
        CC-->>W: {name, state, …}
    end
```

**The subscription is two event types, not one.** The design document says
`["song_changed"]`; the code subscribes to `song_changed` *and*
`state_changed`, and is right to — the Last.fm scrobble timer needs to know
when playback pauses, and the periodic `GET /api/player` reconciliation is a
backstop, not the primary signal.

**Why there is a seed on every connect.** A reconnect means events were missed.
Without the seed, a track that started during the gap would never be enriched,
because no further event announces it. The seed is strictly better than the
in-process bridge this replaced, which left a mid-restart track un-enriched.

**What a gap actually costs.** Everything that happens inside it. A track that
starts *and* ends within a gap is never enriched and never scrobbled. The
per-connect seed recovers the current song and the 30 s reconciliation recovers
state, so the exposure is bounded by the backoff — at most 30 s once the cap is
reached.

**The merge policy on arrival is not part of the seam** and was deliberately
left untouched by the conversion. `apply_song_information` merges partially: it
refuses information for a song that is no longer playing, replaces cover art
only when the stored value is a placeholder, and records provenance in
`metadata.cover_art_source`. A partial the policy refuses, or one that only
restates what the song already says, answers `applied: false` — saying otherwise
would wake every client to re-read an identical song.

**The keep-alive.** The server pings every connection every 30 s. This exists
because a listen-only client used to be pruned after an hour with its socket
still open: the browser WebSocket API exposes no way for JavaScript to send a
ping, so a page that subscribed and then waited simply went deaf, with no error
and nothing in any log. The pong refreshes the client's activity; a peer that
has gone away stops answering and is still reaped.

---

## Seam 2: library enrichment

The metadata half is told when a library changes, enriches what is new, and
posts results back in batches. It is told over seam 1a — the `library_changed`
event — and it also sweeps every library on every connect, because nothing
replays an event missed while the socket was down. A slow periodic sweep remains
as a backstop for an event that was neither delivered nor seeded.

**A reload announces itself twice, and only the second announcement finds
work.** Both backends emit the event as the rebuild starts, when the library
reports `is_loaded: false`; the puller correctly declines to enrich a
half-rebuilt library and does nothing at all. The announcement that matters is
the one made when the load has finished. That is why `mark_loaded_and_announce`
sets the loaded flag and sends the event in a single call on both backends: with
only the first announcement, enrichment would wait for the backstop on every
load.

```mermaid
sequenceDiagram
    autonumber
    participant PLIB as MPD library
    participant WS as api/events → now_playing_ws
    participant PULL as library_puller
    participant CC as CoreClient
    participant SINK as HttpEnrichmentSink
    participant ROUTE as api/enrichment

    Note over PLIB,WS: the load announces itself; nothing is called on the metadata side
    PLIB->>WS: library_changed (loaded, tokens raw)
    WS->>PULL: wake naming "mpd" — no tokens travel

    loop on a wake, on every connect, or every 10 min
        PULL->>CC: GET /api/library
        CC-->>PULL: players with has_library && is_loaded
        PULL->>CC: GET /api/library/mpd
        CC-->>PULL: {library_version, library_generation, …}

        alt library_version unchanged since last enriched
            Note over PULL: nothing to do
        else changed — or 30 min elapsed with no version at all
            PULL->>CC: GET /api/library/mpd/artists
            PULL->>CC: GET /api/library/mpd/albums
            PULL->>PULL: record library_version as seen
            PULL->>SINK: start both sweeps<br/>with ONE library_generation

            loop per batch of ≤ 50 results
                SINK->>ROUTE: POST /api/library/mpd/enrichment<br/>{library_generation, artists, albums}
                alt generation still current
                    ROUTE-->>SINK: 200 {artists, albums, library_version}
                    Note over SINK: record the returned version as seen —<br/>the merge bumped it, and our own<br/>write must not look like a change
                else library reloaded since
                    ROUTE-->>SINK: 409 {library_generation, library_version}
                    Note over SINK: sweep ends; the next pass re-pulls
                else no such player or library
                    ROUTE-->>SINK: 404
                    Note over SINK: sweep ends
                end
            end
        end
    end
```

**Both sweeps get the same generation, and that is the point.** The artist sweep
and the album sweep run concurrently on their own threads. Nothing either posts
moves the *generation*, so neither can make the other's next batch look stale.
If the album sweep ever stops after one batch on a library that also has
artists, that is the signature of the generation having been confused with the
version.

**No conditional requests.** The list ETags are built from `library_version`,
and the fetch happens only *because* that version moved — so an
`If-None-Match` could never be answered 304. The design document originally
specified one; it was removed rather than kept as an unreachable branch.

**An empty batch is not posted.** Posting nothing cannot move the version, so
the seen token stays as the detail route reported it and the library is not
re-pulled.

**A library with no version is re-fetched every 30 minutes.** LMS reports
neither a version nor a generation. Its batches name no generation, which its
backend accepts — the version is *not* substituted.

**Batch size is 50.** The trade is between how soon a client sees a lookup and
how often every client's cached list is invalidated, since a library bumps
`library_version` once per batch that changed something. Sending one result at
a time would invalidate every cached list once per artist.

---

## Seam 3: the resolvers — gone

**There is no seam 3 any more.** The player half asked the metadata half two
pure questions about names, and both are now decided locally and corrected
afterwards.

- **Which half of a split stream title is the artist.** `SongTitleSplitter`
  decides from `forced_order`, then a learned `default_order`, then a fixed
  heuristic. A wrong guess is **not** corrected on the track playing: it is
  reported as a per-station observation to `POST /player/<n>/splitter/
  <station>/observation`, which feeds the learned order for later tracks.
  `song-information` cannot carry it -- that route identifies a song by title
  and artist, and a swap disagrees with both.
- **Whether an album-artist string names one artist or several.** Both library
  loaders split on separators alone — once per album, no network — and the
  correction arrives in the enrichment batch as `split_into`, which is seam 2b.
  The visible cost is that the first load meeting a new album artist shows the
  plain split until the sweep runs; the visible gain is a load that no longer
  makes a MusicBrainz round trip per album.

The routes themselves still exist on the metadata daemon for clients that ask
directly, and `scripts/check-crate-deps.sh` fails the build if their paths
reappear in the player sources outside a comment. What is gone is the player
half calling them.

The exchange it used to be, kept because the encoding rule below outlived it:

```mermaid
sequenceDiagram
    autonumber
    participant SPL as stream title splitter
    participant MC as MetadataClient
    participant R as api/resolve
    participant MB as MusicBrainz

    SPL->>MC: title_order("The Beatles", "Hey Jude")
    MC->>R: GET /api/resolve/title-order?part1=…&part2=…
    R->>MB: which part is the artist?
    MB-->>R: answer, or nothing
    R-->>MC: {"order": "artist_song" | "song_artist" | "unknown" | "undecided"}
    MC-->>SPL: OrderResult

    SPL->>MC: artist_split("Simon, Garfunkel", separators)
    MC->>R: GET /api/resolve/artist-split?name=…<br/>&separator=,&separator=%26&separator=%20feat%20…
    R->>MB: are these real, separate artists?
    MB-->>R: answer, or nothing
    R-->>MC: {"artists": ["Simon","Garfunkel"] | null}
    MC-->>SPL: Option<Vec<String>>
```

**Each separator is its own repeated `separator=` parameter.** This is not
cosmetic. A separator is arbitrary text and `,` is itself the first of
`DEFAULT_ARTIST_SEPARATORS`, so a comma-joined list cannot carry its own
contents: the defaults arrived with the comma destroyed and comma-separated
album artists stopped splitting, while a configured `", "` arrived as `" "` and
split every two-word artist name in the library in two. A separator cannot
share a delimiter with the list that carries it.

**`null` meant one artist**, and was a real answer rather than a failure. That
is the answer `split_into` now carries as a one-element list, for exactly the
same reason: it has to be distinguishable from "no answer".

**Both fell back rather than failing.** An unreachable metadata side gave
`unknown` for the order and a plain separator split for the name — the same
answers a MusicBrainz-disabled build produces, and the same answers both callers
now start from.

---

## Seam 4: the Spotify access token

**This seam runs metadata → player, and it is the only one that changed
direction.** The Spotify account — the OAuth flow, the stored tokens and their
refresh — lives in the player half, because its primary consumer is playback
control: the librespot backend turns `PlayerCommand::Play`, `Pause`, `Next` and
the rest into Spotify Web API calls, at nine call sites. While the token was
fetched across the seam, a metadata half that was down meant pressing play did
nothing, which inverts the priority the split is designed around.

So the player half reads its own account (no HTTP at all), and the metadata
half asks *it* for a token, for the two providers that search Spotify.

```mermaid
sequenceDiagram
    autonumber
    participant PR as coverart / favourites provider
    participant CC as CoreClient
    participant SP as api/spotify (player half)

    PR->>CC: spotify_access_token()
    alt cached and younger than 60 s
        CC-->>PR: Some(token)
    else
        CC->>SP: GET /api/spotify/access_token
        alt an account is linked
            SP-->>CC: 200 text/plain — the token
            CC->>CC: cache it with a 60 s TTL
            CC-->>PR: Some(token)
        else no account, or refresh failed
            SP-->>CC: 404
            CC-->>PR: None
        end
    end
```

`None` means the provider contributes nothing, which is what it already did on
a device with no Spotify account linked. Playback control is unaffected either
way: it never asks over HTTP.

The 60 s cache bounds how often the token is fetched and, equally, how long a
token survives an account being unlinked. A stale token fails at Spotify with
its own 401, so the TTL is about traffic rather than correctness.

**A known limitation, documented in the code.** `acr-http`'s `get_text` maps
every non-2xx to an error string, so a 404 — meaning no account is linked — is
indistinguishable from the player half being unreachable. Both answer `None`,
and a cached token is left alone in either case. The `post_json_status` method
added for the enrichment 409 exists precisely because that information loss
mattered there; extending it to GET was left as unnecessary rather than done
speculatively.

**Search is duplicated, deliberately.** The metadata half holds its own
small Spotify client -- search and a saved-tracks check, no account and no
refresh -- rather than calling the player half's
`POST /api/spotify/search`. Proxying would put rate-limited provider network
work on the player half's Rocket workers and make a cover-art lookup two hops.
What is duplicated is one documented GET, not an abstraction.

---

## Seam 5: favourites

There is no seam. The favourites routes live entirely in the metadata crate and
are served from it; `liked` state reaches the player half as an ordinary field
on seam 1. This is listed for completeness because the design document numbers
it as an interface.

---

## What the player half serves without asking

Two of its own routes used to be answered by calling the metadata half, once
per request. They were the last two calls in that direction and they were not
proxying: neither returns the metadata daemon's answer, they merge it into a
differently shaped one. Both are gone, and each went a different way, because
what they needed is different.

### Artist detail: the answer travels in the batch

`GET /api/library/<p>/artist/by-name|by-id|by-mbid/…` serves an artist's
biography, its source and its banner. Those used to be fetched from
`GET /artist/<b64>` on the metadata half and merged over what the library held.

They now travel in the enrichment batch, as three more fields on
`ArtistSummary` beside the thumbnails that were already there, and
`data::library`'s merge writes them like the rest. Nothing is fetched while a
request is being answered.

**What a client sees:** the same fields in the same shape, and later. Before, an
artist whose sweep had not yet reported could still answer with a biography,
because the route read the metadata side's own cache directly. Now it answers
without one until the batch arrives — the same eventual consistency the genres
and thumbnails in that response have always had, bounded by the same sweep.

The cost is memory: a biography is the largest thing a library holds per artist,
and it is now held on the player side for every artist a sweep has reported.
That is the trade the rule forces, and it is stated here rather than discovered.

### Artist images: the route names where they are

`GET /api/library/<p>/image/artist:<name>` used to fetch
`GET /coverart/artist/<b64>/image` from the metadata half and serve the bytes.
Bytes cannot travel in the batch, and the player half has nothing local to
answer from: artist art is fetched from providers, downloaded on first use and
kept in the metadata half's artist store.

So the route names the destination instead of calling it — **302 to
`/coverart/artist/<b64>/image`**, rewritten for the request's forwarded prefix,
with `size` carried across.

That path is not a new address for a client to learn: it is the same one the
artist lists have always put in `thumb_url`, so a client following the redirect
lands where it would have gone from the list.

**What a client sees:** a 302 where it saw a 200. Every client that follows
redirects — browsers, `URLSession`, `requests`, `curl -L` — sees the same image.
One that follows none sees a 302 body instead of bytes. Two things improve:
`?size=` now works, because the route it points at resizes and this one never
did (`resize_via_cache` answers for `album:` identifiers only), and an unknown
player or a player without a library still answers 404, because the redirect
sits inside the player lookup rather than before it.

### Why not defer both to nginx

The spec's first answer was to route `/api/metadata/` in nginx and let clients
fetch the biography and the image themselves, with the two calls kept as
documented exceptions until the WebUI and `hbos-ios` migrated. That is a real
option and it is why the spec calls them "the main daemon proxying for its own
clients". It was not taken, for two reasons.

It does not remove the calls, so `services.metadata` has to stay, and with it
the address that makes every other violation easy. The rule would hold by
convention on the one path that most invites breaking it.

And it is a larger change for a client than either of these, not a smaller one:
a client would have to make a second request per artist and merge the two
responses itself, where the redirect costs it nothing and the batch costs it
nothing at all.

---

## The two library tokens

This is the subtlest thing in the design, and the one place that produced a
serious bug. Two opaque tokens describe a library, and they are not
interchangeable.

```mermaid
stateDiagram-v2
    direction TB

    [*] --> Loaded: library loads

    Loaded --> Loaded: a merge changed something — VERSION bumps, generation unchanged
    Loaded --> Reloading: library reloads
    Reloading --> Loaded: album map emptied, GENERATION bumps

    note right of Loaded
        library_version: cache validator, and the
        "have I enriched this already?" token.
        Carries a hash of the caller's forwarded prefix.

        library_generation: changes ONLY on reload.
        The token a batch names. Carries NO prefix,
        because it validates nothing a proxy rewrites.
    end note
```

| | `library_version` | `library_generation` |
|---|---|---|
| Changes when | contents change, including by a merge | the library is *reloaded* |
| Used as | ETag validator; the puller's seen-token | the token an enrichment batch names |
| Prefixed | **yes** — a hash of the caller's `X-Forwarded-Prefix` | **no** |
| Absent when | the backend cannot track changes (LMS) | the backend cannot tell it reloaded (LMS) |
| Answered by | `GET /api/library/<p>`, and the enrichment 200/409 | `GET /api/library/<p>`, and the 409 |

**Why the 409 compares the generation and not the version.** A version moves on
every merge — including the batch's own. Two sweeps running against one library
would read each other's bumps as a reload and abort each other. A generation
moves only when the library is rebuilt, which is the one thing that actually
makes a batch unmergeable.

**Why the version is prefixed and the generation is not.** The list bodies
contain paths that depend on the request's prefix, so two different bodies share
one URL; a validator built from library state alone would name both. The
generation validates nothing a proxy rewrites, and a prefixed one would match
nothing the library holds.

**The bug this caused, and the shape of it.** `prefix_tag(None)` hashes the
*empty string* rather than short-circuiting, so the absent-prefix tag is a real
eight-character value and the prefixed version is *never* equal to the raw one —
not even with no proxy in the path. The enrichment route originally returned the
raw version while the detail route returned the prefixed one, and the puller
compared them for equality. Every library with anything to enrich was re-swept
every 30 seconds, forever. The fix was to fold the version at the route, using
the request's own prefix, so the two agree by construction for every caller.

Two things are worth carrying from that. First, the whole test suite missed it
because every fixture used the same literal on both sides of the comparison —
a token test needs the token's real shape. Second, the generation's tokens carry
a `g` marker, so a confusion between the two *refuses* rather than merging
silently.

**One structural guard.** `bump_generation` has exactly one production caller,
`reset_albums_for_reload`, which does the bump and the map clear as a single
operation. Clearing that map by any other route would break what the 409 means.
Private-to-the-module holds that from outside; inside it is a strong convention
and not an enforced one.

There is a second path that ends a library's life: on a connection retry, MPD
puts a *freshly constructed* library into the controller rather than reloading
the existing one. That is safe — a new library carries a new nonce, so a
generation held across the swap cannot match — but safe for a different reason.
Turning that replacement into a reuse of the existing counter, which looks like
an obvious optimisation, reintroduces exactly the stale merge the generation
prevents.

---

## Start-up and shutdown

Two of the metadata daemon's parts — the subscriber and the puller — are
clients of a socket it does not own, so it cannot start them where it starts
everything else. The unit carries `After=audiocontrol.service`, but `After` is
ordering and not readiness: the metadata daemon will often be up before the
player daemon has bound. So it waits for the socket itself.

```mermaid
sequenceDiagram
    autonumber
    participant M as audiocontrol-metadata (main)
    participant IP as initialize_in_process
    participant MRK as its own Rocket (port 1084)
    participant ST as start_after_core_is_listening
    participant RK as audiocontrol (port 1080)
    participant WS as now_playing_ws
    participant PULL as library_puller

    Note over M: refuse to start unless core.url is set —<br/>before a cache is opened or a port is bound
    M->>IP: register providers, read config
    Note over IP: no socket needed — runs early
    M->>MRK: spawn API thread
    MRK->>MRK: bind 127.0.0.1:1084, mount routes
    M->>ST: (after the image purge starts)

    loop until answered, 30 s bound, or a stop signal
        ST->>RK: GET /api/version
    end
    Note over ST: three ways out: answered,<br/>bound expired (start anyway and<br/>let reconnect handle it), or stop
    ST->>WS: start subscriber
    ST->>PULL: start puller
```

**There is deliberately no `Requires=` in either direction.** The player daemon
failing must not stop the metadata daemon retrying, and the metadata daemon
failing must not touch playback. The 30 s bound exists so that a player daemon
that is slow, or absent, costs a delayed start rather than a daemon that never
starts: after it expires the subscriber and the puller start anyway and their
own reconnect and retry logic takes over. A stop signal arriving during the
wait cuts it short, and so does the API server failing to bind — without that,
a metadata daemon that could not have port 1084 would sit in the probe for the
full 30 s before saying so.

Shutdown matters more than it looks. An open WebSocket is pending I/O that
Rocket waits out for its whole grace period, and then spends its mercy period
force-closing. With a client attached — the metadata half's own subscriber, or a
browser on the web interface — that added five seconds to every `systemctl
stop`, restart and package upgrade.

The fix is the server closing the connection itself: the client loop races
Rocket's shutdown signal and sends a Close frame with code 1001. That brings a
stop with a client attached from about five seconds to about two. The remaining
two seconds are the grace period itself, which a WebSocket task pays even after
its socket is closed, because it holds a reference Rocket counts.

Asking the subscriber to stop is a *separate* mechanism and does not save that
time — measured on its own it changes nothing, because the loop notices a stop
only at a wait and spends shutdown parked in a blocking read. What it does buy
is that the subscriber will not reconnect into a daemon that is going away, and
it shares the flag the start-up wait watches — which turns a signal arriving
during start-up from the full eight-second force-exit into about a tenth of a
second.

**Stopping is now two units, and each pays its own shutdown.** The metadata
daemon's is the cheaper of the two: it serves no browser sockets, only its own
outbound subscription, and a `systemctl stop audiocontrol-metadata` on a test
device with an 11,858-album library was **measured at 59 ms**. The player
daemon's stop is unchanged, and is still the one that costs seconds when a
browser is attached.

---

## What clients see

The web interface and `hbos-ios` ship separately from the daemon and meet both
old and new versions. That constrains what may change.

Everything reaches clients through `/api/audiocontrol/`, which nginx proxies to
the player daemon, and through `/api/metadata/`, which it proxies to the
metadata daemon. The metadata routes are mounted **twice** on that daemon: at
their historical paths, and again under `/api/metadata/`. **Which daemon
answers a client's request is nginx's business, not the client's** — no
client-facing URL changed when the process split.

| | Path | Who it is for |
|---|---|---|
| Historical | `/api/coverart/…`, `/api/lastfm/…`, `/api/favourites/…`, `/api/audiodb/…`, `/api/artist/…`, `/api/resolve/…`, `/api/imagecache/external/…` | existing clients — unchanged, and not going to move. nginx forwards exactly these seven sub-prefixes of `/api/audiocontrol/` to 1084; everything else under that prefix goes to 1080 |
| Historical, player daemon | `/api/spotify/…` and every player route | existing clients. Stays on 1080: the account is the player half's |
| New mount | `/api/metadata/…` | the same metadata routes, plus `capabilities`, at the prefix nginx routes to 1084 as a whole. **Not `/api/metadata/spotify/…`** — that path never shipped |
| Loopback only | bare `/api/…` on 127.0.0.1:1080 | the metadata daemon's own subscriber and puller. Not a client-facing surface |

**One sub-prefix is load-bearing rather than cosmetic.** `/api/coverart/` has
to reach the metadata daemon, because it is where
`/api/library/<p>/image/artist:<name>` sends clients and where the artist
lists' `thumb_url` points. A deployment that routes `/api/metadata/` and
forgets `/api/coverart/` loses every artist image on the device, with the
player daemon answering a 302 into its own SPA fallback.

`GET /api/metadata/capabilities` is the one path that exists *only* under the
new mount. The player half serves its own `/api/capabilities`, and two
identical routes at one base make Rocket refuse to ignite — the daemon does not
start at all. That collision is why the route was given the second mount and
not the first; a test asserts the two route sets stay disjoint, so the mistake
surfaces as a red test rather than as a device that will not boot. Now that the
two are separate processes it could not collide in practice any more, but the
player daemon still mounts both sets when built with default features, so the
constraint is kept.

**The two `images.sizes` ladders must agree.** Each daemon reads its own
configuration file, `/api/capabilities` reports the player daemon's list and
`/api/metadata/capabilities` the metadata daemon's. A daemon that disagreed
would miss the other's cached variants and regenerate them under a second name.
Nothing checks this at run time; the shipped `audiocontrol.json` and
`metadata.json` both carry the same ladder and both say so in a comment.

**Compatibility promises that this phase changed:**

- **A client no longer needs a periodic send to stay connected** (from 0.22.0).
  The server pings. Keeping a periodic send is harmless and is still required
  against older daemons.
- **A shutting-down daemon sends a Close frame with code 1001** (from 0.22.0).
  Earlier daemons let the socket be torn down, which clients saw as code 1006.

That second one carries a real risk, and it is the inverse of the obvious one.
A client that treats a Close as a fault merely reports an error where it used to
report a dropped connection. But this idiom is common:

```js
socket.onclose = (e) => { if (!e.wasClean) reconnect(); };
```

Against every earlier daemon a shutdown produced `1006` and `wasClean: false`,
so this reconnected. Against 0.22.0 it receives a *clean* close and stops
reconnecting — across exactly the events where reconnecting matters most, a
restart or a package upgrade. **Reconnect on every close.**

---

## When something is missing or slow

| Situation | What happens | What a user sees | What a log shows |
|---|---|---|---|
| Metadata daemon down or not yet started | **playback works**, and so does every player route: nothing in the player daemon waits on it. Verified on hardware by stopping the unit and playing | no enrichment, no scrobbling — and **artist images stop serving too**, including ones already on disk: the redirect target `/api/coverart/artist/<b64>/image` is the metadata daemon's route, so nginx has no upstream for it. This is a new failure mode rather than a worsened one: in one process there was no such state, because a metadata failure took playback with it. The split buys playback that survives, and pays for it with artist images that do not | nothing on the player side — it has nothing to fail |
| `services.metadata` present in a config file | **ignored.** Nothing reads it, and no client is built from it | nothing | nothing |
| `core.url` absent or misspelled in `metadata.json` | **the metadata daemon refuses to start**, before it opens a cache or binds a port | metadata stops working; playback does not | one error naming the key, and the same on stderr |
| Player daemon unreachable, from the metadata daemon | the subscriber and puller retry with backoff to 30 s; results in flight are dropped | enrichment and scrobbling go stale; playback unaffected | one warning naming the URL, then a reminder every 5 min |
| Socket dropped | reconnect with backoff; a seed recovers the current song | a track starting *and* ending inside the gap is never enriched or scrobbled | debug per attempt |
| Batch computed against a reloaded library | 409 ends the sweep; the next poll re-pulls | a short delay before enrichment reappears | debug |
| Batch names a library that is gone | 404 ends the sweep | nothing | one line |
| Library reports no version (LMS) | re-fetched every 30 minutes instead | enrichment lags a change by up to 30 min | nothing |
| Library reports no generation (LMS) | batches name none, and the backend accepts them | nothing | nothing |
| No secrets compiled in | providers needing a key are disabled at start-up | those providers contribute nothing | one line per absent secret |
| Unknown field in a batch | 422 — the batch is refused rather than silently applied unchecked | nothing | one line |

The first row is the whole point of the phase, and it is the one property here
that is asserted against two real processes rather than argued for: the Python
integration suite starts both daemons, stops the metadata unit and sends a play
command. The second row is what the one-way seam cost. `services.metadata` used
to mean "there is a metadata side, here is where": absent, the daemon quietly
lost enrichment, the resolvers, artist detail and artist images. There is
nothing to say now, because the player half does not call and cannot.

The third row is not symmetrical with the second, and the asymmetry is
deliberate. `get_service_config` falls back to the top level, and
`metadata.json` puts `webserver` there with port 1084 — so a `metadata.json`
that omits or misspells `core.url` would derive `http://127.0.0.1:1084/api`,
the metadata daemon's *own* port. It would then subscribe to its own event
stream and poll its own library, neither of which it serves, and fail silently
forever with a reminder every five minutes. The deriving default is right in
one process and wrong in two, so the metadata daemon refuses to start without
an explicit `core.url` rather than running on talking to itself.

---

## What the split changed, and what it cost

It was packaging and configuration, as the spec said it would be. Nothing in
the seams above changed shape.

| | How it is |
|---|---|
| Processes | two |
| Binaries | two, **in one package**, enabled, upgraded and rolled back together |
| systemd units | two: `audiocontrol.service` and `audiocontrol-metadata.service` |
| Configuration | two files: `audiocontrol.json` and `metadata.json` |
| Ports | 1080 for the player daemon, 1084 for the metadata daemon |
| `services.core.url` | `http://127.0.0.1:1080/api`, in `metadata.json` — **the only address either daemon holds**, and it must be written out rather than derived |
| Client paths | unchanged; nginx routes `/api/metadata/` and seven sub-prefixes of `/api/audiocontrol/` to 1084 |
| Caches | separate: the metadata daemon has its own image cache, settings database and lookup cache under `/var/lib/audiocontrol/metadata` |
| Credentials | separate: the player daemon's `/var/lib/audiocontrol/security_store.json` holds the Spotify keys, the metadata daemon's `/var/lib/audiocontrol/metadata/security_store.json` holds `lastfm_session_key` and `lastfm_username`. `postinst` copies, never moves, so a rollback finds both files intact |

There is no row for `services.metadata`: it is gone, and that is what made the
split a packaging change. Only one daemon has to be told where the other is, so
only one configuration file names an address.

**The credential stores are split because two processes cannot share one.**
`SecurityStore::save_to_file` serialises its whole in-memory map over the file
with `File::create`, which truncates. Two daemons that each loaded a copy at
start-up and each wrote it back would lose whatever the other had written since
— a Spotify refresh overwritten by a Last.fm authentication, silently, with the
symptom being an account that re-authenticates forever. Splitting the file
removes the question instead of managing it. Each store keeps the other's keys
after the migration copy; they are inert, and deleting them was deliberately
not attempted, because a partial migration that removed a key and then failed
would unlink an account.

### What the seam actually costs

The spec was candid that this was unmeasured: "Everything above is reasoning
about round trips that have never crossed a real socket boundary in this
system." It has now crossed one. The figures below were **measured on a test
device with an 11,858-album library — 211,475 songs, 10,927 artists as MPD
reports them, loading to 11,858 albums and 1,919 artists on the player side.**
Everything not marked as measured is an estimate, and says so.

| What | Measured | On |
|---|---|---|
| Player-side library load | **463.24 s** for 211,475 songs | one MPD load, cold |
| `GET /artists` across the seam | **0.27 MB**, **27 ms** | three runs, ±1 ms |
| `GET /albums` across the seam | **2.65 MB**, **98 ms** | three runs, ±2 ms |
| Metadata daemon RSS | **10.8 MB at rest → 31.4 MB peak** | while pulling the library |
| Player daemon RSS | **313 MB peak**, settling to about **130 MB** | during and after the library load |
| Metadata daemon shutdown | **59 ms** | `systemctl stop` |

**The seam is not the expensive part, and it does not need streaming or
paging.** That was the open question: the puller reads `/artists` and `/albums`
whole and materialises the response string, then a `serde_json::Value`, then
the parsed `Vec` — three copies of one body alive at once — and the spec asked
whether a multi-megabyte spike per sweep meant the seam had to be redesigned.
Measured, the larger of the two bodies is 2.65 MB, so the three copies are
about **8 MB transient**, which is consistent with the daemon's observed rise
from 10.8 MB to 31.4 MB while it pulls. On a 20,000-album library the same
arithmetic gives roughly 4.5 MB a copy. Against a device that may have one
gigabyte, and against the 463 s the library load itself takes, that is not
where the money goes. Streaming or paging the lists would be work spent on the
cheap end of the phase.

What the numbers do *not* cover: this is one library, on one device, with MPD
as the backend. Nothing here says how the seam behaves on a library several
times larger, or on LMS, and nothing measures a sweep running concurrently with
a reload.

**Two smaller costs, as predicted and not separately measured.** A cover-art
lookup asks for a Spotify token first if none is cached; the 60 s TTL bounds
it, and a negative answer is cached too, so an unlinked account costs one call
a minute rather than one per lookup. And shutdown is two units, each paying its
own grace period — see [Start-up and shutdown](#start-up-and-shutdown).

---

## Where this map is thin

Written down because a document that only describes what works is not a map.

- **`enhance_metadata: false` no longer disables library enrichment.** The thing
  it guarded — `request_enrichment` — is deleted, so on LMS the flag now
  suppresses nothing whatever and is only *reported* by the library's metadata
  map; on MPD it still gates image pre-warming and nothing else. The puller
  sweeps every loaded library regardless, and it has no way to be told not to.
  The design document said this flag becomes a no-op and is removed from the
  docs; it is now thoroughly the first and still not the second.
- **The metadata daemon does not serve `/api/version`.** Its `route_groups`
  mounts the metadata routes and nothing else, so there is no way to ask it
  which release it is over HTTP; `/api/version` on 1080 answers for the player
  daemon only. The two always ship as one package, so the question has one
  answer — but a health check that wants to confirm the metadata daemon is
  running has to ask something else, such as `GET /api/metadata/capabilities`.
  The daemon's own start-up probe of `GET /api/version` goes the other way,
  to 1080.
- **The artist-split cache is keyed on the artist name alone.** The separators
  are not part of the key and entries never expire, so changing
  `artist_separators` in configuration does not invalidate what is already
  cached. It now only affects the metadata half's own answer, which is offered
  as a claim the loader may already have got right — but a wrong cached entry is
  still a wrong claim, applied to every album under that name.
- **A configured `artist_separator` does not cross the seam, and a batch can
  override it.** The removed route took the list as repeated `separator=`
  parameters; nothing replaces that channel, so `split_observation` only ever
  sees the built-in list. It refuses to claim anything about a name holding none
  of those, which keeps it from rejoining a custom split of a name like
  `Alpha|Beta` — but that gate is shaped around the *name*, not around whether a
  custom list is configured, so it does not hold in the other direction.
  Confirmed on a device with `artist_separator: ["|"]`: a batch claiming
  `split_into: ["Alpha", "Beta"]` for a name that also holds a built-in
  separator rewrote the album's artist list, overriding the operator's list.
  **The separators should cross the seam again.** `GET /api/library/<p>` already
  reports per-player detail and is the obvious place to carry the configured
  list; until it does, the gate is a partial defence and this is a regression
  against the removed route rather than a neutral simplification.
- **LMS albums are now offered for genre lookup, and never used to be.**
  `request_enrichment` on that backend deliberately offered artists only, on the
  grounds that LMS albums carry whatever the server reports and a MusicBrainz
  request per album would be new cost on a backend that had never made one. The
  puller does not know that: it offers every player's albums that carry no
  genres. This is not new here — the puller has been the only live discovery
  path since the nudge went — but the test that documented the intent went with
  `request_enrichment`, so it is recorded here instead. The fix, if it is wanted,
  is a per-player flag in `GET /api/library/<p>` rather than a rule in the
  puller.
- **`EnrichmentSink`'s error type cannot express a transport failure**, so a
  dropped request is reported as "the library is gone" and abandons the sweep.
  It was harmless while both halves shared a process. Now that a request can
  fail because the other daemon restarted, one dropped request costs a whole
  sweep until the next poll — up to ten minutes on the backstop.
- **Three routes do synchronous, rate-limited provider work on request
  threads** — `resolve/title-order`, `resolve/artist-split`, and artist detail
  with `?lookup=true`. Concurrent uncached requests can occupy every Rocket
  worker. The player half no longer calls any of them, so what is exposed is a
  client's own request. A fourth is now reachable the same way:
  `/coverart/artist/<b64>/image` *downloads* on a miss, and the artist image
  redirect sends every client that asks the player half for artist art straight
  at it. That was already true of `thumb_url`, so the redirect widens an
  existing path rather than opening one.
- **A biography is held per artist on the player side now**, bounded at 2000
  bytes. It is the largest thing in an `ArtistMeta` and the only one no list
  shows, carried because the artist detail routes serve it and may no longer
  fetch it. Unbounded it looked like the largest new cost in the phase, on an
  *estimate* from per-artist byte counts: roughly three times the stored text
  once allocation is counted, so ten thousand artists at three kilobytes would
  be about ninety megabytes on a device that may have one gigabyte and is
  holding the library too. `summarise` applies `sanitize::safe_truncate` at the
  one point every provider's text crosses, which brings the estimate to about
  six kilobytes an artist, or around sixty megabytes for ten thousand — set
  against **255 MB**, which is what the whole daemon then measured, in one
  process, holding a 200,000-song library. The full text is unaffected on the
  metadata side. What a user sees is a long biography cut mid-sentence in the
  artist detail field.

  **That estimate has still not been tested at the scale it worries about.**
  The device the phase was measured on loads 1,919 artists on the player side,
  a fifth of the ten thousand the arithmetic assumes, and its player daemon
  settles at about 130 MB. A library with ten thousand *player-side* artists
  would be the test, and there has not been one.
- **Two memory figures appear in this document, and they measure different
  things.** The 255 MB in the bullet above is one number for one process
  holding both halves. The figures under
  [what the seam actually costs](#what-the-seam-actually-costs) are per daemon
  and were taken after the split, on a 211,475-song library: the player daemon
  peaks at 313 MB during the load and settles to about 130 MB, and the metadata
  daemon sits at 10.8 MB, rising to 31.4 MB while it pulls. So the newer pair
  supersedes the older single number rather than contradicting it — the same
  work now has two resident sets, and the larger one is the library, which was
  always the bulk of it. They are not a controlled comparison: different
  libraries, different sample points, and the older figure was a steady state
  where 313 MB is a load-time peak. What can be said is that the player daemon
  alone, after the split, is well under what one process holding both halves
  was.
- **`?lookup=true` has no test** distinguishing "lookup skipped" from "lookup
  ran and found nothing", because no provider is registered in the unit test
  environment.

---

## See also

- [architecture.md](architecture.md) — the whole system, of which this is one seam
- [api.md](api.md) — every route, with request and response shapes
- [websocket.md](websocket.md) — the event contract and its compatibility rules
- [specs/2026-09-08-two-processes.md](specs/2026-09-08-two-processes.md) — the phase that made them two processes: the credential hazard, the packaging, and the measurement it asked for
- [specs/2026-09-07-one-way-seam.md](specs/2026-09-07-one-way-seam.md) — why the seam runs one way, and what each removed call was replaced with
- [specs/2026-09-04-player-metadata-split.md](specs/2026-09-04-player-metadata-split.md) — the split this sits inside, partly superseded by the two above
- [tooling.md](tooling.md) — building and testing in a container
