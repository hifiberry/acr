"""The two-process arrangement, against two real processes.

Phase 2 of the player/metadata split starts the metadata half as its own
daemon: the player daemon on 1080, the metadata daemon on 1084, every
connection between them opened by the metadata side. The suite runs them on
18080 and 18084. `doc/specs/2026-09-08-two-processes.md` names three things
that need asserting once they are two processes rather than one, and this file
is those three:

1. **Playback works with the metadata daemon stopped.** The property the whole
   phase exists to deliver. Enrichment, cover art and scrobbling stop; play,
   volume, library and the WebSocket must not.
2. **A write to one daemon's credential store leaves the other's alone.**
   `SecurityStore::save_to_file` serialises the whole in-memory map and
   truncates, so two processes sharing one file would have the second writer
   erase what the first had learned. The resolution was a file per daemon, and
   this is that file boundary under two live processes.
3. **A `metadata.json` with no `core.url` must fail to start**, saying so.
   `get_service_config` falls back to the top level, where `webserver` sits
   with the metadata daemon's own port, so the absent key would otherwise
   derive an address for *itself*: it would subscribe to its own event stream
   and poll its own library, serving neither, and fail silently forever.

**Which build these run against, and why it matters.** The fixture uses the
default `cargo build --workspace` binaries. The *packaged* player daemon is
built `--no-default-features --features alsa` and serves no metadata routes at
all; the default build carries an in-process metadata shadow that answers
`/api/coverart/methods`, `/api/favourites/providers`, `/api/lastfm/status`,
`/api/metadata/capabilities` and `/api/resolve/title-order` from its own
state. A test that passed only because that shadow answered would be worse
than no test. So nothing here asks the player daemon for a metadata route:
case 1 asserts player-owned routes only, and it proves the metadata daemon is
genuinely down by requiring its own port to refuse connections first, which no
shadow in the other process can fake. `integration_test/README.md` records the
choice and its cost.

**No credentials are involved.** Case 2 exercises the store *files* -- which
daemon owns which, and what a truncating rewrite reaches -- with placeholder
values that are not credentials and are never decrypted, and it asserts on key
names and file digests, never on a value. What it therefore does not cover is
a provider login: no Spotify OAuth exchange, no Last.fm session key issued by
Last.fm, and so not the real refresh that would race a real authentication.
That end of it is covered by unit tests on both sides and, per the spec, by
the upgrade test on a device.
"""

import json
import time

import pytest
import requests
import websocket

from conftest import (
    LASTFM_KEYS,
    METADATA_DAEMON_CONFIG,
    SPOTIFY_KEYS,
    TEST_PORTS,
    MetadataDaemonTestServer,
    digest,
    run_metadata_daemon_to_completion,
    security_store_keys,
)


def stop_the_metadata_daemon(two_daemons):
    """Stop the metadata daemon and prove it is down before asserting anything.

    "Down" has to be established rather than assumed. A graceful shutdown takes
    a moment, and a request that arrived during it would be answered -- so the
    port has to refuse a connection before the rest of a test means anything.
    """
    two_daemons.metadata.stop()
    two_daemons.metadata.wait_until_unreachable(timeout=15)


def test_playback_answers_with_the_metadata_daemon_stopped(two_daemons):
    """The play command, volume, the library and now-playing, with the other
    daemon gone.

    This is the row of the failure matrix in `doc/communications.md` that the
    split turns from hypothetical into real, and the reason the phase was worth
    doing: the player daemon holds no address for the metadata daemon, so
    losing it must cost enrichment and nothing else.

    Every route here belongs to the player half. None of them is one the
    default build's metadata shadow answers, so a green result says the player
    daemon served them itself.
    """
    stop_the_metadata_daemon(two_daemons)

    base = two_daemons.player.server_url + "/api"

    r = requests.post(f"{base}/player/test/command/play", timeout=10)
    assert r.status_code == 200, f"the play command must still answer: {r.text}"
    assert r.json()["success"] is True

    now_playing = requests.get(f"{base}/now-playing", timeout=10)
    assert now_playing.status_code == 200
    assert now_playing.json()["player"]["name"] == "test"

    volume = requests.get(f"{base}/volume/info", timeout=10)
    assert volume.status_code == 200
    # Whether a control is *available* depends on the machine -- there is no
    # ALSA mixer in a container -- so what is asserted is that the volume API
    # answers for itself rather than what it answers.
    assert "available" in volume.json()

    library = requests.get(f"{base}/library", timeout=10)
    assert library.status_code == 200


def test_the_event_websocket_still_delivers_with_the_metadata_daemon_stopped(two_daemons):
    """`/api/events` keeps serving clients when its biggest subscriber is gone.

    The metadata daemon is a WebSocket client of this route, and the WebUI and
    the phone clients are others. A subscriber that disappears must not take
    the stream down with it, so this connects after the metadata daemon is
    stopped and requires a real event to arrive.
    """
    stop_the_metadata_daemon(two_daemons)

    port = TEST_PORTS['two_daemons']
    ws = websocket.create_connection(f"ws://127.0.0.1:{port}/api/events", timeout=15)
    try:
        welcome = json.loads(ws.recv())
        assert welcome["type"] == "welcome", f"expected a welcome frame, got {welcome}"

        r = requests.post(
            f"http://127.0.0.1:{port}/api/player/test/update",
            json={"type": "state_changed", "state": "playing"},
            timeout=10,
        )
        r.raise_for_status()

        # Bounded, and it reports what did arrive: an empty list means the
        # stream went quiet, a list without `state_changed` means the event
        # vocabulary moved.
        deadline = time.time() + 20
        seen = []
        while time.time() < deadline:
            ws.settimeout(max(0.5, deadline - time.time()))
            try:
                message = json.loads(ws.recv())
            except websocket.WebSocketTimeoutException:
                break
            seen.append(message.get("type"))
            if message.get("type") == "state_changed":
                assert message["state"] == "playing"
                return

        pytest.fail(
            "no state_changed event arrived on /api/events within 20s with the "
            f"metadata daemon stopped; frames seen: {seen}"
        )
    finally:
        ws.close()


def test_a_credential_store_write_leaves_the_other_daemons_store_intact(two_daemons):
    """Each daemon writes its own store file and cannot reach the other's.

    The hazard the phase was designed around. `SecurityStore::save_to_file`
    serialises the whole in-memory map and writes it with a truncating
    `File::create`, so a process that writes a file it does not own erases
    whatever the owner has learned since that process last loaded it -- with no
    error, no lock and no merge. One file, two writers, and a Spotify refresh
    or a Last.fm session key goes missing; the symptom is an account that
    re-authenticates forever, or unlinks itself.

    Both stores are seeded before the daemons start (each loads its store once,
    at start-up), then each daemon is made to write:

    * `POST /api/spotify/tokens` on the player daemon stores the access token,
      the refresh token and the expiry. It reaches no network -- `store_tokens`
      writes three keys and returns.
    * `POST /api/lastfm/disconnect` on the metadata daemon removes the session
      key and the username. It needs no account: the client is initialised from
      `services.lastfm.enable`, and `SecurityStore::remove` writes the file
      because the seeded keys are there to remove.

    **What this does not do is log in anywhere.** The values written are
    placeholders, the seeded ones are not valid ciphertext and are never
    decrypted, and every assertion is about key names and file digests. So this
    covers the file boundary that makes the hazard impossible; it does not
    cover a real OAuth refresh racing a real Last.fm authentication, which
    needs credentials no test may hold.
    """
    player_store = two_daemons.player_store
    metadata_store = two_daemons.metadata_store

    assert player_store != metadata_store, (
        "the fixture must give the two daemons separate store files, or nothing "
        "below is a test of anything"
    )
    metadata_before = digest(metadata_store)

    # The player daemon writes.
    r = requests.post(
        f"{two_daemons.player.server_url}/api/spotify/tokens",
        json={
            "access_token": "placeholder-not-a-credential",
            "refresh_token": "placeholder-not-a-credential",
            "expires_in": 3600,
        },
        timeout=10,
    )
    assert r.status_code == 200
    assert r.json()["status"] == "success", (
        "the player daemon could not write its credential store. A "
        "--no-default-features binary cannot: SecurityStore::initialize_with_"
        "defaults is behind the metadata feature. Rebuild with default "
        f"features. Response: {r.json()['message']}"
    )

    player_after_own_write = digest(player_store)
    # A superset rather than an exact list: after an upgrade the player
    # daemon's store legitimately holds Last.fm's keys too, because postinst
    # copies the store rather than splitting it and the foreign entries are
    # left inert. What matters is that its own five are there.
    assert set(SPOTIFY_KEYS) <= set(security_store_keys(player_store)), (
        "the player daemon's own keys must survive its own write"
    )
    assert digest(metadata_store) == metadata_before, (
        "the player daemon's store write reached the metadata daemon's file"
    )
    assert set(LASTFM_KEYS) <= set(security_store_keys(metadata_store))

    # The metadata daemon writes.
    r = requests.post(
        f"{two_daemons.metadata.server_url}/api/lastfm/disconnect", timeout=10
    )
    assert r.status_code == 200
    assert digest(metadata_store) != metadata_before, (
        "the metadata daemon did not write its store at all, so this test would "
        "pass whatever the player daemon's file did next"
    )
    assert not set(LASTFM_KEYS) & set(security_store_keys(metadata_store)), (
        "disconnect must remove the keys it owns from its own store"
    )

    assert digest(player_store) == player_after_own_write, (
        "the metadata daemon's store write reached the player daemon's file: "
        "the two daemons are sharing one credential store"
    )
    assert set(SPOTIFY_KEYS) <= set(security_store_keys(player_store))


def metadata_config_without_core_url(work_dir):
    """The shipped test config with `core.url` taken out, written to a file.

    Derived from the real template rather than hand-written, so that the config
    the daemon refuses is the config it otherwise accepts, differing in exactly
    the one key.
    """
    with open(METADATA_DAEMON_CONFIG, 'r') as f:
        config = json.load(f)

    config["services"]["webserver"]["port"] = TEST_PORTS['two_daemons_metadata']
    del config["services"]["core"]["url"]

    path = work_dir / "metadata-without-core-url.json"
    work_dir.mkdir(parents=True, exist_ok=True)
    with open(path, 'w') as f:
        json.dump(config, f, indent=2)
    return path


def test_the_metadata_daemon_refuses_to_start_without_core_url(tmp_path):
    """No `core.url`, no daemon -- and it says which key is missing.

    Without the gate this is the quietest failure in the split.
    `get_service_config` falls back to the top level, `webserver` is there with
    this daemon's own port, and the derived `core.url` would be
    `http://127.0.0.1:<own port>/api`. The daemon would then subscribe to its
    own event stream and poll its own library, serving neither, and go on
    failing forever with a reminder every five minutes.

    A non-zero exit alone would not be evidence: a missing file, an unparseable
    file and an occupied port all exit non-zero too, and this daemon now exits
    non-zero whenever it cannot serve. So the assertion is on the message
    naming the key. Its falsification is the next test, which supplies the key
    to the same config and watches the daemon start.
    """
    config_path = metadata_config_without_core_url(tmp_path)

    returncode, stderr = run_metadata_daemon_to_completion(config_path, tmp_path)

    assert returncode is not None, (
        "the metadata daemon started with no core.url; it must refuse to"
    )
    assert returncode != 0, f"expected a non-zero exit, got {returncode}"
    assert "core.url" in stderr, (
        "the failure must name the key that is missing, or the operator is left "
        f"guessing between this and a bad path or a bound port. stderr: {stderr!r}"
    )


def test_the_same_configuration_starts_once_core_url_is_supplied(tmp_path):
    """The falsification of the test above, as a test.

    The same file, plus `core.url`, must start -- otherwise the refusal above
    proves nothing about that key: a daemon that could not start on this
    configuration for some other reason would produce the same non-zero exit.

    Nothing is listening on the `core.url` named here and nothing needs to be.
    The seam is one-way and start-up does not wait on the player daemon
    answering: the daemon binds its own port first, then probes `GET
    /api/version` for up to 30 s and starts the subscriber and the library
    puller anyway.
    """
    config_path = metadata_config_without_core_url(tmp_path)
    with open(config_path, 'r') as f:
        config = json.load(f)
    config["services"]["core"]["url"] = f"http://127.0.0.1:{TEST_PORTS['two_daemons']}/api"
    with open(config_path, 'w') as f:
        json.dump(config, f, indent=2)

    daemon = MetadataDaemonTestServer(
        port=TEST_PORTS['two_daemons_metadata'],
        core_port=TEST_PORTS['two_daemons'],
        work_dir=tmp_path,
        # Exactly the file written above -- the point of the test is that this
        # one file, with the key restored, starts.
        config_path=config_path,
    )

    try:
        assert daemon.start(), (
            "with core.url supplied the daemon must serve its own port; if this "
            "fails the refusal test above is not about core.url"
        )
    finally:
        daemon.stop()
