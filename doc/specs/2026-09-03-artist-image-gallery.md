# Several images per artist, and one of them chosen

Status: Implemented

## The problem

An artist can hold exactly one picture. `get_artist_user_image_path` formats
`{user_dir}/artists/{sanitised}/{type}.jpg`, and the lookup in
`get_cached_image` takes the first of user `custom` → user `cover` → cache
`custom` → cache `cover`. Every write goes to one of those two user files, so
a second upload for an artist overwrites the first and the first is gone.

That is visible in three places today:

- `POST /coverart/artist/<b64>/update` downloads the chosen URL over
  `custom.jpg`. Picking a different provider image discards the previous one.
- The upload endpoint on this branch writes `custom.jpg` too, and its request
  body is a map keyed by artist name — a shape that cannot express two images
  for one artist even in principle.
- `GET /coverart/artist/<b64>` returns several provider candidates with
  grades, and the WebUI already offers them for selection. The set is
  transient: nothing keeps a candidate after another one is chosen.

What is wanted is a set of images per artist that persists, that the WebUI can
show and pick from, and that uploads add to rather than replace.

## Scope

In scope:

- Storing more than one image per artist, from uploads and from what the
  daemon has already downloaded.
- Listing that set, serving any member of it, and deleting a member.
- Recording which member is selected, reusing the mechanism the WebUI already
  drives.
- Replacing the unreleased batch upload endpoint with a single-image one.

Out of scope:

- Persisting live provider candidates that were never downloaded. A candidate
  becomes a member of the set by being selected, which downloads it, exactly
  as today.
- Any change to how providers are queried, graded or ranked.
- Album and song artwork. Only artist images have a user directory of this
  shape.

## Storage

Uploads land in a directory of their own beneath the artist:

```
{user_dir}/artists/{sanitised}/custom.jpg          # unchanged: URL download
{user_dir}/artists/{sanitised}/cover.jpg           # unchanged: auto-download
{user_dir}/artists/{sanitised}/uploads/{md5}.{ext} # new
```

`{md5}` is the MD5 of the stored bytes and `{ext}` comes from sniffing those
bytes, not from anything the client said — the same rule the external cover
art module already applies, and the reason the serving route can derive a
content type from the name.

Content addressing buys idempotence: uploading bytes that are already stored
resolves to the same file, so a client that retries after a timeout does not
grow the set, and the resized variants generated from those bytes stay valid
because the bytes behind the name never change.

Uploads sit in a subdirectory rather than beside `custom.jpg` because
`remove_artist_image_variants` reads the artist directory and matches
`<stem>@<size>` entries against a stem. Keeping uploads and their variants in
`uploads/` leaves that logic looking at one flat directory of known names.

## Identity

An image's id is its file stem: `custom`, `cover`, or the 32-character MD5 of
an upload. Ids are unique within an artist — an upload id is always 32 hex
characters and can never collide with `custom` or `cover`.

Ids are opaque to the client: it reads them from the listing and passes them
back. Nothing decodes them.

## API

### List

`GET /coverart/artist/<artist_b64>/images`

```json
{
  "images": [
    { "id": "custom", "url": "/api/coverart/artist/<b64>/image/custom",
      "source": "download", "selected": true,
      "width": 640, "height": 640, "size_bytes": 51234 },
    { "id": "8f14e45fceea167a5a36dedd4bea2543", "url": "…/image/8f14…",
      "source": "upload", "selected": false,
      "width": 1400, "height": 1400, "size_bytes": 402113 }
  ]
}
```

`source` is `download` for `custom` and `cover`, `upload` for the rest.
Dimensions come from the existing header sniffer in `image_meta`, which reads
the first bytes of a file rather than decoding it, so listing an artist costs
one `stat` and one short read per image. An unreadable or unrecognisable file
is omitted from the listing and logged, rather than failing the request.

`image_meta::image_size` caches its answer keyed by the path, with no expiry.
That is safe for an upload, whose name is its content hash and whose bytes
therefore never change, but not for `custom.jpg` and `cover.jpg`, which keep
their names when a new download replaces them: those measurements must be
dropped with `image_meta::clear_image_cache` wherever those two files are
written, or the listing will report the previous image's dimensions for ever.

An artist with no images answers `{"images": []}` with 200. Absence of images
is not an error, and a client asking "what is there" gets an answer.

### Serve one

`GET /coverart/artist/<artist_b64>/image/<id>?<size>`

Serves that image, `404` when the id is not in the set. `size` behaves exactly
as on the existing route: it snaps up the ladder and generates the variant
beside the original on first use.

The existing `GET /coverart/artist/<artist_b64>/image?<size>` is unchanged and
keeps serving whichever image is selected, so a client that knows nothing
about this feature keeps working.

### Upload

`POST /coverart/artist/<artist_b64>/upload`

```json
{ "image_base64": "…" }
```

One image, for the artist named in the path — the same path grammar as
`/update`. The bytes are decoded and validated within the daemon's existing
decode limits before anything is written, stored under `uploads/`, and then
selected, because uploading a picture is a request to use it. The previously
selected image stays in the set.

The response reports the id, so the client can address the image it just
uploaded without listing again:

```json
{ "success": true, "id": "8f14e45fceea167a5a36dedd4bea2543",
  "message": "Stored image for 'The Beatles'" }
```

This replaces `POST /coverart/artists/upload`, which is removed. That endpoint
has not appeared in a release, so no compatibility path is owed. Its batch
framing is what forced the request body to be keyed by artist name, and that
keying is what makes it unable to express this feature.

### Delete

`DELETE /coverart/artist/<artist_b64>/image/<id>`

Removes the file and its variants. Deleting the selected image clears the
selection, and the lookup falls back to the existing chain — so deleting an
upload that was selected reveals `custom.jpg` again, and deleting all of them
leaves the artist with whatever the providers next download. `cover` may be
deleted like any other member; auto-download may recreate it later, which is
the behaviour a user asking for a fresh download expects.

`404` when the id is not in the set, so a double delete is reported honestly
rather than as a success.

## Selection

Selection stays where it is: `POST /coverart/artist/<artist_b64>/update` with
a URL, recorded in the settings database under `artist.image.{artist_name}`.
The WebUI already drives this endpoint for provider candidates, and a member
of the set is just another URL it can post.

What changes is that the daemon recognises its own URLs. A URL of the form
`{API_PREFIX}/coverart/artist/<b64>/image/<id>` naming the same artist is
resolved to that member and recorded as the selection; nothing is downloaded
and no bytes are copied. Any other URL keeps today's behaviour: download to
`custom.jpg`, which is itself a member of the set.

A URL that looks local but names a different artist, or an id that is not in
that artist's set, is refused with a message saying so. The daemon never
fetches its own address over HTTP to satisfy this.

Two consequences for the lookup:

- `get_cached_image` consults the selection before the `custom` → `cover`
  precedence, otherwise selecting an upload while `custom.jpg` exists would
  have no effect.
- The store's in-memory `image_cache` map must lose the artist's entry
  whenever the selection changes, an image is uploaded, or an image is
  deleted. It is keyed by artist name and holds a resolved path, so a stale
  entry would serve the previous image until a restart.

An empty URL clears the selection, as it does today.

## Limits

- **Ten uploads per artist.** An upload that would exceed the cap is refused
  with a message naming the cap, rather than evicting the oldest: silently
  discarding a picture a user chose is worse than telling them the set is
  full. Re-uploading bytes that are already stored resolves to the existing id
  and does not count against the cap.
- **Request body.** With one image per request, the batch justification for
  raising Rocket's global `json` limit to 10 MiB is gone. The limit is a
  single global for every JSON route in the daemon, so it should be no larger
  than the largest single request the daemon means to accept: 4 MiB, which
  covers a lossless PNG of an artist portrait with room to spare, and still
  bounds the memory a single request can make the daemon allocate.
- **Decode limits** are unchanged: the existing `imageresize::validate` bounds
  the decode before an image is accepted.

## Authorisation

Three new paths need entries in `auth.d/audiocontrol-auth.json`, which
otherwise falls through to `default_tier: risky` and answers 401 on a gated
device:

- `GET /coverart/artist/*/images` and `GET /coverart/artist/*/image/*` join
  the unauthenticated GET tier. The existing `/coverart/*/*/*` pattern already
  covers the first but not the second, which is one segment longer.
- `POST /coverart/artist/*/upload` joins the POST tier beside
  `/coverart/artist/*/update`, which already stores an image for an artist.
- `DELETE /coverart/artist/*/image/*` joins the DELETE tier. It removes user
  data, which is the one operation here that is not recoverable by asking the
  providers again — it is listed with the others rather than left to the risky
  default only because the WebUI is the client and reaches the daemon the same
  way it does for `/update`.

## Compatibility

- Files already on a device appear in the listing with no migration:
  `custom.jpg` and `cover.jpg` are members from the first request.
- A client that only knows `/artist/<b64>/image` and `/update` sees no change.
- The WebUI ships separately and will meet both the old and the new daemon, so
  nothing here removes a route it depends on. The one removal,
  `/coverart/artists/upload`, has never been released.

## What the tests should be

Pure, no filesystem:

- An id is derived from the bytes and is stable across two encodings of the
  same image; a different image yields a different id.
- A local image URL is recognised for the right artist, and refused for
  another artist, for an unknown id, and for a remote host that merely ends in
  the same path.
- The cap counts stored uploads, and a re-upload of existing bytes does not
  count against it.

Against a temporary user directory:

- An upload is stored under `uploads/`, is listed with `source: upload`, and
  is selected immediately; the previously selected image is still listed.
- Uploading the same bytes twice yields one member, one id and no second file.
- A selection pointing at an upload wins over an existing `custom.jpg`.
- Deleting the selected image clears the selection and the lookup falls back
  to `custom.jpg`; deleting a member removes its variants too.
- The in-memory path cache does not serve the previous image after an upload,
  a selection or a delete.
- Listing an artist directory containing a file that is not an image omits it
  rather than failing the request.

Against the API, in the style the crate already uses for coverart routes:

- The upload response carries the id, and that id serves the bytes back.
- `GET …/image/<unknown>` and `DELETE …/image/<unknown>` answer 404.
- The unchanged `GET …/image` serves whatever is selected.
