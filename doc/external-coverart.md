# External cover art providers

Cover art can be fetched from HTTP endpoints named in the configuration, in
addition to the built-in providers (Spotify, Last.fm, TheAudioDB, fanart.tv).
An endpoint is a config entry, not a code change, so several can be configured
at once.

These endpoints are assumed **slow**. The first one written against this
contract is an LLM-backed lookup that takes 20-40 seconds. Nothing that a
client waits on ever waits for them by default.

## Configuration

Under `services` in `audiocontrol.json`:

```json
"external_coverart": {
  "enable": true,
  "providers": [{
    "name": "llm",
    "display_name": "AI Lookup",
    "url": "https://tools.example.com/coverart?artist={artist}&title={title}",
    "methods": ["song"],
    "headers": { "Authorization": "Bearer ..." },
    "timeout_seconds": 45,
    "trigger": "fallback",
    "cache_ttl_days": 30,
    "negative_cache_ttl_days": 7,
    "max_concurrent": 1,
    "localize": false,
    "max_image_bytes": 4194304
  }]
}
```

| Field | Default | Meaning |
|---|---|---|
| `name` | required | Identifies the endpoint. It is the cache key prefix and the value clients see in `song.metadata.cover_art_source`, so it must be unique and is worth choosing well. |
| `display_name` | the `name` | Shown by `GET /api/coverart/methods`. |
| `url` | required | Template; see placeholders below. |
| `methods` | `["song"]` | Any of `artist`, `song`, `album`, `url`. |
| `headers` | none | Sent verbatim. Credentials go here, as they do for the other services in this file. |
| `timeout_seconds` | 45 | How long a single lookup may take. |
| `trigger` | `fallback` | See below. |
| `cache_ttl_days` | 30 | How long an answer is kept. |
| `negative_cache_ttl_days` | 7 | How long "there is no artwork" is kept. |
| `max_concurrent` | 1 | Lookups in flight at once. A background lookup that cannot get a slot is abandoned; an `include_slow` request waits for one, bounded by `timeout_seconds`. |
| `localize` | `false` | Fetch this endpoint's `url` images into the image cache and serve them from the daemon instead of handing the endpoint's URL to clients. Inline images are always stored locally, whatever this says. See [below](#serving-the-images-ourselves). |
| `max_image_bytes` | 4194304 (4 MiB) | The largest single image accepted, inline or fetched. Clamped to 64 MiB, which is how much of one response the daemon may hold in memory at a time. An inline image has a lower ceiling than this key can raise; see [why the default is 4 MiB](#why-max_image_bytes-defaults-to-4-mib). |

A malformed entry is skipped with a warning; it does not stop the daemon or
the other endpoints.

### Placeholders

`{artist}`, `{album}`, `{title}`, `{year}` and `{url}` are substituted and
percent-encoded. A placeholder the lookup has no value for becomes empty.

| Lookup | `{artist}` | `{album}` | `{title}` | `{year}` | `{url}` |
|---|---|---|---|---|---|
| artist | artist | — | — | — | — |
| song | artist | — | song title | — | — |
| album | artist | album title | — | year, if known | — |
| url | — | — | — | — | the source URL |

For an album lookup the album's name is in `{album}`; `{title}` is a song
title and is empty.

### `trigger`

`fallback` (the default) asks only when the song has no artwork or only a
placeholder, such as a radio station's logo. `always` asks for every song.

`trigger` governs only the background lookup the daemon makes on its own for
the song currently playing. It has no effect on `GET /api/coverart/...`: a
request made with `?include_slow=true` is an explicit instruction and is
always honoured, regardless of `trigger`.

**On the now-playing path, `trigger` controls cost, not outcome.** The daemon
never replaces artwork that belongs to a song — only a placeholder is
replaceable — so an answer for a song that already has real artwork is
discarded whatever `trigger` says. Setting `trigger: "fallback"` to limit
spend on a paid endpoint reduces how often the background lookup runs; it
does not restrict `?include_slow=true`, which asks unconditionally.

## The contract

The daemon sends a `GET` with the configured headers and expects `200` with:

```json
{
  "images": [
    { "url": "https://images.example.com/cover.jpg" },
    { "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB..." }
  ]
}
```

An entry names its image in one of two ways, and needs one of them to be
usable at all:

- `url` — where the image can be fetched.
- `data` — the image itself, base64-encoded. This is the preferred channel for
  an endpoint whose images are not publicly fetchable: it costs one round trip
  instead of two, needs no second authentication, and works even when the
  image host is unreachable from the daemon as well. A missing pad is
  accepted.

If both are present, `data` is used and the `url` is never fetched: the bytes
are already in hand.

`width`, `height`, `size_bytes` and `format` may be sent and are accepted, but
are ignored today: the daemon measures images itself because its grader needs
dimensions it can trust, and it sniffs the format from the bytes it holds, so
a label it does not need is a label that could disagree with them. They are
part of the contract so a later change can use them without a protocol
revision.

Order matters: list the best artwork first. At most the first eight entries of
one answer are considered. That is a bound rather than a setting: taking the
bytes of an image is per-image work — a fetch, a file, and a `stat` on every
later cache read — and since the service has already ranked the list, keeping
the first few loses nothing the grader would have picked anyway.

There is deliberately no way to configure how a response is parsed. Adapting a
service that speaks something else is the job of a small wrapper in front of
it, which keeps the daemon free of an expression language and keeps failures
legible.

### The three answers

| Response | Meaning | Cached for |
|---|---|---|
| `200`, non-empty `images` | Artwork | `cache_ttl_days` |
| `200`, `images: []` | Looked, found nothing | `negative_cache_ttl_days` |
| Anything else | Fault | 1 hour |

"Anything else" covers a non-2xx status, a timeout, a connection failure, and
a body that does not parse as the shape above. A fault is cached briefly so a
broken service is not hammered, and never long, so an outage cannot blank a
track for weeks.

An answer whose `images` all lack both a usable `url` and usable `data` counts
as "found nothing": re-asking would return the same list. An answer that did
name artwork the daemon then could not serve is a fault rather than either of
those; see [when it does not work](#when-it-does-not-work).

## Serving the images ourselves

An endpoint's image URLs reach three consumers: the client rendering
`cover_art_url`, the grader's metadata fetch, and the artist store's download.
A URL on a private network, or one whose images need a credential the client
does not hold, satisfies none of them. The `data` field and `localize` are the
two ways out of that: the daemon ends up holding the bytes, stores them in its
own image cache, and hands out a URL it serves itself.

Bytes and URLs are treated differently, because only a URL can be passed
through:

| Entry | `localize: false` | `localize: true` |
|---|---|---|
| `data` only | Stored locally | Stored locally |
| `url` only | Passed through unchanged | Fetched, then stored locally |
| both | `data` wins: stored locally, no fetch | `data` wins: stored locally, no fetch |

Inline bytes are stored whatever `localize` says, because they have no URL to
pass through. `localize` is off by default because the opposite would be a
regression for the common case: a publicly reachable provider's URLs already
work, and copying its images onto an appliance's disk to serve them again
spends disk to no end. Turn it on for an endpoint on a private network, or one
whose images need a credential the client does not hold.

The configured `headers` are sent on the image fetch **only when the image is
on the endpoint's own origin** — the same scheme, host and port as `url`. An
image elsewhere is still fetched, because naming a CDN is a reasonable thing
for an endpoint to do, but it is fetched without credentials. If your images
need the credential, serve them from the same origin as the endpoint.

The reason is that the address being fetched came out of the endpoint's
*answer*, and an answer is not trusted here: its sizes are bounded, its count
is capped, and an image's type is read from its bytes rather than believed.
Sending the configured token to whatever host an answer happened to name
would hand the credential to anyone who could influence that response.

A localised image is served at
`/api/imagecache/external/<endpoint>/<hash>.<ext>`. Like every other internal
URL the daemon hands out, it is rewritten for `X-Forwarded-Prefix`, both on
the `song_information_update` event and on the now-playing REST response, so a
client behind a reverse proxy gets a URL it can reach.

Two parts of that path are worth naming. The endpoint's `name` becomes the
directory, reduced to `[A-Za-z0-9_-]` first — it is administrator-controlled
rather than attacker-controlled, but a `/` or a `..` in it would put files
somewhere other than the image cache. The extension comes from the bytes and
from nothing the endpoint said, because it is what the image endpoint derives
the `Content-Type` from. The hash is of the query and the entry's position in
the answer, not of the bytes, so a re-lookup of the same track overwrites its
own files instead of accumulating a copy per lookup.

The file is written with an expiry of the endpoint's `cache_ttl_days`, which
is recorded against it — but nothing sweeps the image cache on a schedule
today, so in practice a localised image stays until something deletes it. Plan
for that when turning `localize` on: the cache grows by up to eight files per
distinct lookup, not per lookup, because the path is derived from the query
and a repeat overwrites. A library of a few thousand tracks is therefore
bounded and modest; an endpoint answering `url` lookups for a long tail of
internet radio metadata is not.

Nothing depends on the expiry being enforced. A cached answer whose files have
gone is treated as a miss and looked up again, so deleting the cache directory
is safe at any time.

### Why `max_image_bytes` defaults to 4 MiB

An inline image travels base64-encoded inside the JSON body, and that body is
read with a 10 MiB limit. Base64 expands by 4/3, so an inline image cannot
exceed about 7.5 MiB however `max_image_bytes` is set — and exceeding the body
limit fails the *whole lookup*, not one image, which is then cached as an
error and retried within the hour. Raising this key does not lift that
ceiling, so a default above it would have promised a size the daemon cannot
accept. A fetched `url` image does not travel through the body and can use the
full configured value.

### When it does not work

Bytes that are not a recognised image are refused rather than stored. This is
validation as much as it is naming: a `url` fetch answering with an HTML login
page or a JSON error is a likely failure, and storing those bytes would serve
clients a broken image from a URL the daemon vouches for. JPEG, PNG, GIF, WebP
and BMP are recognised; anything else is refused.

A single image that cannot be served — invalid base64, over
`max_image_bytes`, a fetch that failed, unrecognised bytes, a write that
failed — is dropped with a warning, and the images that did work are still
artwork. But an answer that loses *every* image is cached as a fault for an
hour, not as an absence of artwork for `negative_cache_ttl_days`: the endpoint
did report artwork, so failing to obtain it is a fault on this side of the
exchange, and it must not blank the track for a week.

The answer cache and the image cache expire independently in practice — a
cleared cache directory, a manual delete — so a cached answer whose local
files have gone is treated as a miss and the track is looked up again, rather
than served as a URL that 404s. A cached *error* still short-circuits; that is
the whole reason errors are cached. An external URL in a cached answer is kept
unexamined: it is not the daemon's to check, and a mixed answer is possible
when an endpoint sends one image inline and another as a public URL.

## How answers reach clients

A lookup started for the song being played publishes a
`song_information_update` when it finishes, with
`song.metadata.cover_art_source` set to the endpoint's `name`. This may arrive
after playback has moved to the next song; the daemon drops an answer that no
longer matches. See [the event contract](websocket.md).

`GET /api/coverart/...` returns cached answers from these endpoints but never
waits for one, unless called with `?include_slow=true`. Use that only where
someone asked for the lookup and can see that it is running.
