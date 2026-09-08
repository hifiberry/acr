"""
Integration tests for the player/metadata HTTP seams (Phase 1 of the
player/metadata split).

These exercise the daemon started from `test_config_metadata.json`, in which
`services.core` points back at the daemon's own port -- in this phase the two
halves share one process, so what the metadata side calls is a loopback call to
itself. **There is no `services.metadata`**, and that is the point of this
suite as much as anything it asserts: the daemon ignites without one, because
nothing in it addresses the metadata daemon. See `doc/communications.md`,
`doc/architecture.md` and `doc/specs/2026-09-07-one-way-seam.md`; each test
below is pinned to one seam, named in its docstring.

**Library enrichment is not observable here at all.** The configured player is
`generic`, which has no library, so nothing sweeps and nothing is enriched. The
config therefore leaves `library_poll_seconds` unset, at the production
backstop of 600s, rather than the 30s it used to pin: a 30s backstop in a suite
this length could carry a test past a broken event path, and there is no test
here for it to carry. The event path end to end is exercised by running a real
daemon against a real MPD.

What this suite does **not** cover, so that nobody reads a green run as more
than it is. The test config enables no metadata provider -- no MusicBrainz, no
TheAudioDB, no Spotify, and Last.fm with `now_playing_enabled: false` -- which
is deliberate, because a provider that reaches the network makes the answers
non-deterministic. The daemon therefore logs "No now-playing enrichment is
configured" on start-up, and no worker consumes the WebSocket subscriber's
events here. The subscriber and the library puller *do* start (visible as
"library puller started" and "WebSocket client registered (id: 0)"), so the
seam is live and its start-up path is exercised; what is not exercised is a
lookup travelling from a provider back through `POST
/player/<name>/song-information`. That path is covered by unit tests on both
sides of the seam, and end to end only by running a real daemon with real
credentials.
"""

import requests


def test_song_information_reaches_now_playing(metadata_server):
    """Interface 1, push direction: a partial song posted to
    `POST /player/<name>/song-information` is merged by
    `apply_song_information` and shows up on `GET /api/now-playing`, with
    `cover_art_source` recording that the cover art came from an enrichment
    lookup rather than the player itself."""
    base = metadata_server.server_url + "/api"

    r = requests.post(
        f"{base}/player/test/update",
        json={"type": "song_changed", "song": {"title": "Nemo", "artist": "Nightwish"}},
    )
    r.raise_for_status()

    r = requests.post(
        f"{base}/player/test/song-information",
        json={"title": "Nemo", "artist": "Nightwish", "cover_art_url": "http://example/cover.jpg"},
    )
    assert r.status_code == 200
    assert r.json()["applied"] is True

    song = requests.get(f"{base}/now-playing").json()["song"]
    assert song["cover_art_url"] == "http://example/cover.jpg"
    assert song["metadata"]["cover_art_source"] == "enrichment"


def test_stale_song_information_is_not_applied(metadata_server):
    """The same route refusing a song that is not the one currently playing.

    A song must actually be playing for this to test the identity check
    (`apply_song_information`'s title/artist comparison) rather than its
    separate "no song playing at all" early return -- the brief's own version
    of this test posted straight to a freshly started daemon with no song
    ever announced, which took that other branch and would have passed even
    with the identity check deleted entirely. Announcing "Here" first and
    then sending information for "Elsewhere" is what actually exercises the
    mismatch. A 200 with `applied: false` is correct; a merge would be the
    bug."""
    base = metadata_server.server_url + "/api"

    r = requests.post(
        f"{base}/player/test/update",
        json={"type": "song_changed", "song": {"title": "Here", "artist": "Nightwish"}},
    )
    r.raise_for_status()

    r = requests.post(
        f"{base}/player/test/song-information",
        json={"title": "Elsewhere", "artist": "Nightwish", "cover_art_url": "http://example/x.jpg"},
    )
    assert r.status_code == 200
    assert r.json()["applied"] is False


def test_metadata_mount_answers_capabilities(metadata_server):
    """`/api/metadata/capabilities` exists at all. It is served only from the
    second mount (`/api/metadata`), because the player daemon serves its own
    `/api/capabilities` and two identical routes at the bare `/api` prefix
    would make Rocket refuse to ignite."""
    r = requests.get(metadata_server.server_url + "/api/metadata/capabilities")
    assert r.status_code == 200
    assert "images" in r.json()


def test_resolve_artist_split_without_musicbrainz_is_plain(metadata_server):
    """Interface 3 over the client-facing mount: with MusicBrainz disabled
    (`services.musicbrainz.enable: false` in the test config) the artist
    split falls back to the plain separator split, so the answer is
    deterministic."""
    r = requests.get(
        metadata_server.server_url + "/api/metadata/resolve/artist-split",
        params={"name": "A & B"},
    )
    assert r.status_code == 200
    assert r.json()["artists"] == ["A", "B"]


def test_default_separators_survive_the_query_string(metadata_server):
    """The encoding regression this route shipped with, over the real wire.

    `MetadataClient` sends one `separator=` per separator. It first sent them
    joined by a comma -- and `,` is itself the first of the daemon's default
    separators, so the list could not survive carrying itself: comma-separated
    artists stopped splitting, and a configured `", "` arrived as `" "` and
    split every two-word artist name in two. Names here are unique to this
    test because the split cache is keyed on the name alone."""
    base = metadata_server.server_url + "/api/metadata"
    defaults = [",", "&", " feat ", " feat.", " featuring ", " with "]

    r = requests.get(
        f"{base}/resolve/artist-split",
        params={"name": "Wire Alpha, Wire Beta", "separator": defaults},
    )
    assert r.status_code == 200
    assert r.json()["artists"] == ["Wire Alpha", "Wire Beta"]

    # A separator that contains the old delimiter must stay one separator.
    r = requests.get(
        f"{base}/resolve/artist-split",
        params={"name": "Wirecomma Pink Floyd", "separator": [", "]},
    )
    assert r.json()["artists"] is None


def test_current_player_reports_a_state(metadata_server):
    """The field the scrobble timer reconciles against every 30 s (Task 2).
    No route was added for it -- `GET /api/player` already answered it before
    this phase -- so nothing else in this suite would notice its shape
    changing out from under that reconciliation."""
    base = metadata_server.server_url + "/api"

    r = requests.post(
        f"{base}/player/test/update",
        json={"type": "state_changed", "state": "playing"},
    )
    r.raise_for_status()

    body = requests.get(f"{base}/player").json()
    assert body["state"] == "playing"
    assert body["name"] == "test"


def test_the_daemon_runs_with_no_metadata_service_configured(metadata_server):
    """The one-way seam's whole claim, asserted where a config file can break
    it: this daemon holds no address for the metadata daemon.

    Every other test here runs against the same daemon, so all of them depend
    on it having started; this one says so out loud and adds the half that
    ignition alone does not prove -- that the metadata routes still answer.
    Before the seam went one way, removing `services.metadata` cost artist
    detail and artist images; now it costs nothing, because nothing reads it.
    """
    with open(__file__.rsplit("/", 1)[0] + "/test_config_metadata.json") as f:
        config = f.read()
    assert '"metadata": {' not in config, (
        "this suite is only evidence while the config has no services.metadata"
    )

    r = requests.get(metadata_server.server_url + "/api/metadata/capabilities")
    assert r.status_code == 200


def test_the_artist_image_redirect_target_is_a_real_route(metadata_server):
    """`/api/library/<p>/image/artist:<name>` answers 302 to
    `/api/coverart/artist/<b64>/image` rather than fetching from it.

    A redirect is only as good as what it points at, and the failure mode is
    quiet: a `Location` naming a route that does not exist gives the client a
    404 one hop later, which looks like a missing image rather than a broken
    daemon. So this asserts the target route is *mounted* -- an unknown artist
    is refused by the handler, with the handler's own message, not by Rocket
    having no such route.
    """
    # "bm9ib2R5" is "nobody" base64url-encoded.
    r = requests.get(
        metadata_server.server_url + "/api/coverart/artist/bm9ib2R5/image"
    )
    assert r.status_code == 404
    assert "No image found for artist" in r.text, (
        "the route must answer this itself; Rocket's own 404 for an unmounted "
        f"path would not say it. Got: {r.text!r}"
    )
