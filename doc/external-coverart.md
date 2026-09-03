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
    "max_concurrent": 1
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
| `max_concurrent` | 1 | Lookups in flight at once. A lookup that cannot get a slot is abandoned, not queued. |

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

**On the now-playing path, `trigger` controls cost, not outcome.** The daemon
never replaces artwork that belongs to a song — only a placeholder is
replaceable — so an answer for a song that already has real artwork is
discarded whatever `trigger` says. `always` changes what is returned only for
the REST endpoints and for artist images, where no such policy applies.

## The contract

The daemon sends a `GET` with the configured headers and expects `200` with:

```json
{
  "images": [
    { "url": "https://images.example.com/cover.jpg" }
  ]
}
```

`url` is the only field read. `width`, `height`, `size_bytes` and `format` may
be sent and are accepted, but are ignored today: the daemon measures images
itself because its grader needs dimensions it can trust. They are part of the
contract so a later change can use them without a protocol revision.

Order matters: list the best artwork first.

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

An answer whose `images` all lack a usable `url` counts as "found nothing":
re-asking would return the same list.

## How answers reach clients

A lookup started for the song being played publishes a
`song_information_update` when it finishes, with
`song.metadata.cover_art_source` set to the endpoint's `name`. This may arrive
after playback has moved to the next song; the daemon drops an answer that no
longer matches. See [the event contract](websocket.md).

`GET /api/coverart/...` returns cached answers from these endpoints but never
waits for one, unless called with `?include_slow=true`. Use that only where
someone asked for the lookup and can see that it is running.
