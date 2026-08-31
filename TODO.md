## Client-facing API

Raised while building the iOS client's library browser, and measured against a
real library on the test device on 2026-08-31 (acr 0.9.x): **11,318 albums,
1,904 artists, 181,312 tracks**. Full measurements in the iOS client's
library-browsing design notes. Ordered by how much each saves a client.

* **Serve resized images — `GET /library/<player>/image/<id>?size=<px>`.**
  Cover art is full size only: mean **243 KB** at 1280×1280 (12 sampled, range
  41–379 KB). Downsampling one to 360 px — the 3× size of a 120 pt grid cell —
  takes it from 392 KB to **18.8 KB, 21× smaller**. Across 11,318 albums that
  is a client caching ~213 MB of thumbnails instead of 2.8 GB of originals.

  Every client pays this, not just the new one: `hbos-ui` renders the same
  album grids from the same full-size images, with no `srcset` and no
  client-side resizing. Doing it once here removes a thumbnail cache, an
  off-main-thread decode path and a cache cap from every client that will
  exist. Cost is real: nothing decodes images today (`helpers/imagecache`
  stores bytes and metadata), so this needs a decoding dependency. The
  variants belong in that cache, keyed by identifier plus size.

* **Send cache validators on images.** They carry only `Content-Length` — no
  `ETag`, no `Last-Modified`, no `Cache-Control`. Album art addressed by album
  id never changes, so `Cache-Control: max-age=31536000, immutable` would let
  `URLCache` and browsers hold it themselves. That is one header, and it
  deletes a cache layer from every client — the best value per effort here.

* **Expose a library version, and use it as an `ETag` on the list endpoints.**
  The only change signal today is the counts on `/library/<player>`, which miss
  any edit leaving totals unchanged, so client invalidation can only be a
  heuristic. A monotonic version would make it exact; as an `ETag` on
  `/albums` and `/artists`, revalidation would cost a 304 of a few hundred
  bytes rather than re-sending the list (799 KB gzipped for albums).

  Worth building on the validator work above rather than beside it, so there is
  one `NotModified` path rather than two.

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

### Not acr — nginx, in hifiberryos

* **`/api/*` misses should 404 rather than fall through to the SPA.**
  `GET /api/library/mpd/image/album:<id>` — the un-prefixed path above —
  answers **HTTP 200 with 429 bytes of `text/html`**, the WebUI's `index.html`.
  A client trusting the status code stores the index page as an album cover,
  and nothing reports an error. Any `/api/` path matching no service should
  404.
