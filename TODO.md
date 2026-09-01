## Client-facing API

Raised while building the iOS client's library browser, and measured against a
real library on the test device on 2026-08-31 (acr 0.9.x): **11,318 albums,
1,904 artists, 181,312 tracks**. Full measurements in the iOS client's
library-browsing design notes. Ordered by how much each saves a client.

* **Expose a library version, and use it as an `ETag` on the list endpoints.**
  The only change signal today is the counts on `/library/<player>`, which miss
  any edit leaving totals unchanged, so client invalidation can only be a
  heuristic. A monotonic version would make it exact; as an `ETag` on
  `/albums` and `/artists`, revalidation would cost a 304 of a few hundred
  bytes rather than re-sending the list (799 KB gzipped for albums).

  The image endpoints already answer `If-None-Match` with a 304, so the
  `NotModified` shape exists to copy rather than invent - see
  `src/api/imageresponse.rs`.

* **Emit image paths clients can use unmodified.** Library payloads give
  internal paths — an album has `"cover_art": "/api/library/mpd/image/…"`, an
  artist `"thumb_url": ["/api/coverart/artist/<b64>/image"]` — both missing the
  `/api/audiocontrol` prefix nginx routes on, while `now-playing` **does**
  include it in `song.cover_art_url`. One client then needs two rules depending
  on which endpoint a path came from. Either prefix is defensible; the
  inconsistency is what costs.

  The asymmetry is that `rewrite_api_relative_url` has exactly one caller, in
  `api/players.rs`, and only `song.cover_art_url` goes through it; no library
  endpoint takes the `ForwardedPrefix` guard.

* **Make `?size=` work outside MPD album ids.** Resizing shipped in 0.11.0,
  but it only applies to `album:` identifiers on a player that keeps cover art
  in acr's image cache. MPD does; LMS fetches art over HTTP and never
  populates it, so an LMS client is told `images.sizes` by `/capabilities` and
  then silently gets full-size originals. `artist:` identifiers, bare track
  URLs and URL-safe base64 identifiers miss it too, because the API layer
  matches on the raw path segment while MPD decodes internally. Documented in
  `doc/api.md` rather than hidden, but a client still cannot discover it per
  player. Resizing in memory on a miss would close all four cases at once and
  was rejected for now: an uncached decode costs 300-600 ms on a Pi, so the
  fix needs a cache for those paths, not just a call.

### Not acr — nginx, in hifiberryos

* **`/api/*` misses should 404 rather than fall through to the SPA.**
  `GET /api/library/mpd/image/album:<id>` — the un-prefixed path above —
  answers **HTTP 200 with 429 bytes of `text/html`**, the WebUI's `index.html`.
  A client trusting the status code stores the index page as an album cover,
  and nothing reports an error. Any `/api/` path matching no service should
  404.
