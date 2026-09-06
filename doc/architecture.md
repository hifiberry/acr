# Architecture

This document gives a system-level overview of AudioControl (ACR): what it is, how its
pieces fit together, and how a few concrete events flow through it. For per-subsystem
detail, see the other documents in this directory (linked from [README.md](README.md)).

## What it is

AudioControl is the successor to `audiocontrol2`, HiFiBerry's original Python control
daemon. The rewrite trades dynamic typing for a trait-based architecture: every audio
backend implements one `PlayerController` trait, every state change flows through one
typed event bus, and the plugin system is a small, statically-checked set of
`ActionPlugin`s.

In the wider HiFiBerry OS stack, ACR sits directly on top of PipeWire (which arbitrates
the sound card) and directly under the WebUI, which it serves itself. Audio players ship
as independent Debian packages; ACR is what makes them look like a single, switchable
device to the WebUI and to any external controller. A `generic` backend accepts the same
push API used internally, so a third-party player can register and report state without
a Rust code change.

## System overview

Nine source types feed player-backend modules behind a common trait. The
`AudioController` owns which one is "active" while the `EventBus` fans state changes out
to plugins and API clients. `acr-webmcp` is a separate Python process that turns the
REST API into MCP tools for AI assistants; nginx fronts both for LAN/WAN access.

![ACR system architecture diagram](architecture.svg)

Reading it top to bottom: each source feeds its matching ingestion module in
`src/players/*` (or `src/inputs` for the USB remote). All eight player modules
(everything except the keyboard input) implement the same `PlayerController` trait and
register with the `AudioController`, which tracks which one is currently active, and
publish events onto the `EventBus`. The `ActiveMonitor` plugin listens to that bus and
switches the active player automatically — there is no manual "switch source" step. The
Rocket API layer serves REST, WebSocket, and the static WebUI on port 1080;
`acr-webmcp` calls that same REST API to expose MCP tools on port 13180; nginx proxies
both on port 80.

The eighth player module, `players::generic`, is different from the other seven: it has
no daemon of its own. It is a two-way bridge that any external player can drive over
plain REST — see [Generic backend interface](#generic-backend-interface) below.

## Core abstractions

| Abstraction | Location | Role |
|---|---|---|
| `PlayerController` | `src/players/player_controller.rs` | One trait every backend implements: `get_song`, `get_playback_state`, `send_command`, `get_capabilities`. A shared `BaseController` supplies the `notify_*` helpers that publish to the EventBus, so backends never touch it directly. |
| `AudioController` | `src/audiocontrol/audiocontrol.rs` | Holds every registered controller in a `Vec` plus one `active_index`, and itself implements `PlayerController` by delegating to whichever entry is active — a composite that lets API code treat "the system" as one player. |
| `EventBus` | `src/audiocontrol/eventbus.rs` | Typed pub/sub for `PlayerEvent` (state, song, queue, volume, capabilities…). Every player, plugin, and the WebSocket layer subscribe independently — nothing polls anything else inside the process. |
| `ActionPlugin` | `src/plugins/action_plugin.rs` | Subscribes to the bus and reacts. `ActiveMonitor` is the one that matters most: any player that starts *Playing* automatically becomes the active player. |

## Player backends

Configured under `players` in `/etc/audiocontrol/audiocontrol.json`. "Push" backends are
notified when something changes; "poll" backends are asked.

| Backend | Source | Mechanism | Address | Direction |
|---|---|---|---|---|
| `mpd` | MPD server | MPD binary protocol | `localhost:6600` | poll |
| `librespot` | librespot process | process watch + `--onevent` hook → REST | `POST /player/librespot/update` | push |
| `raat` | Roon Bridge | named pipes | `/var/lib/raat/{metadata,control}_pipe` | push |
| `lms` | Lyrion/Logitech Media Server | JSON-RPC, autodiscovery | `:9000` | poll |
| `bluetooth` | paired BT device | D-Bus (BlueZ `MediaPlayer1`) | system bus | poll |
| `mpris` | any MPRIS2 app (e.g. VLC) | D-Bus, 1s poll | session bus | poll |
| `shairport` | shairport-sync | UDP metadata protocol, listened in-process | `:5555` | push |
| `generic` | any third-party player | same REST push API, by name | `POST /player/<name>/update` | push |

## Generic backend interface

`players::generic` (`GenericPlayerController`, `src/players/generic/generic_controller.rs`)
is the escape hatch for anything that isn't one of the seven named backends: it holds no
connection to a real daemon, just whatever state the last API call gave it. One instance
is created per configured player name, so several independent bridges (e.g. one per room)
can run at once.

![How the generic player backend works](generic-interface.svg)

Two flows run through it, independently and in opposite directions:

- **Inbound (state in).** The bridge `POST`s to `/api/player/<name>/update` with one of
  six event types: `state_changed`, `song_changed`, `position_changed`,
  `shuffle_changed`, `loop_mode_changed`, `queue_changed`. These update the controller's
  internal state and flow out through the normal `notify_*()` → `EventBus` path, exactly
  like any other backend. While the state is `Playing`, `get_position()` interpolates
  from the last reported position using wall-clock time, so the bridge does not need to
  push `position_changed` every second.
- **Outbound (commands out).** When `AudioController` dispatches a `PlayerCommand` to
  this controller — `Play`, `Pause`, `Stop`, `Next`, `Previous`, `Seek`,
  `SetLoopMode`, `SetRandom` — it updates its own state immediately, and *if* a
  `command_url` was configured for this player, also fires a `POST {"command": ...}` to
  that URL on a background thread with a 2-second timeout. The result is not
  awaited or checked: a slow or absent bridge cannot block playback control for
  everything else.

Full request/response shapes for both directions are in
[`src/players/generic/API.md`](../src/players/generic/API.md) and
[Generic Player Controller](generic_player_controller.md).

## Module map (`src/`)

| Module | Responsibility | Key files |
|---|---|---|
| `api/` | Rocket server and every REST/WebSocket route, grouped by domain (players, library, volume, coverart, lastfm, spotify, genres, settings…). | `server.rs`, `players.rs`, `events.rs`, `library.rs` |
| `audiocontrol/` | The engine: `AudioController` (player registry + active selection) and `EventBus` (pub/sub). `now_playing_bridge.rs` is the whole of what the player side knows about metadata enrichment: it forwards song and state changes into a channel and applies what comes back through `apply_song_information`. | `audiocontrol.rs`, `eventbus.rs`, `now_playing_bridge.rs` |
| `players/` | `PlayerController` trait, shared `BaseController`, the eight backend implementations, a JSON-driven factory, and the generic push endpoint. | `player_controller.rs`, `player_factory.rs`, `event_api.rs`, `mpd/`, `librespot/`, `raat/`, `lms/`, `bluetooth/`, `mpris/`, `shairport/`, `generic/` |
| `data/` | Shared domain types passed between every layer: `Song`, `Track`, `PlayerCommand`, `PlayerEvent`, `PlaybackState`, capability sets. | `song.rs`, `player_command.rs`, `player_event.rs`, `capabilities.rs` |
| `helpers/` | Cross-cutting services that stay with the player daemon: volume control and its configurator client, lyrics, m3u parsing, the stream-title splitter, local cover art and image prewarm, and the systemd/MPRIS/Bluez/mac-address process helpers. The SQLite caches, the provider clients and the secret store moved out to the shared crates and `audiocontrol-metadata` — see the module map below. | `volume.rs`, `global_volume.rs`, `configurator.rs`, `lyrics.rs`, `songtitlesplitter.rs`, `local_coverart.rs`, `imageprewarm.rs` |
| `plugins/` | `ActionPlugin` trait plus the built-ins that react to bus events: active-player switching and structured event logging. Last.fm scrobbling is no longer one of them — the `lastfm` entry configures a worker in `audiocontrol-metadata`, and `worker_descriptor.rs` is what keeps that entry in `/api/plugins/actions`. | `action_plugin.rs`, `action_plugins/active_monitor.rs`, `event_logger.rs`, `worker_descriptor.rs` |
| `inputs/` | Hardware input, deliberately separate from streaming players: USB HID remotes turn into `Action`s and reach the controller through an `ActionSink`, so a new rotary or IR source needs no new dispatch code. | `mod.rs`, `keyboard/evdev_source.rs`, `dispatch.rs` |
| `tools/` | 10 standalone `acr_*` binaries — integration hooks, CLI clients, and diagnostics. Four more (`audiocontrol_dump_cache`, `audiocontrol_dump_store`, `audiocontrol_favourites`, `audiocontrol_musicbrainz_client`) build from `crates/audiocontrol-metadata/src/bin/` instead, since they use only metadata-crate code. See [CLI Tools](cli_tools.md). | `src/tools/*.rs` |

## Module map (`crates/`)

Shared code and the metadata daemon's library live in a Cargo workspace next to
`src/`. Five crates are owned by neither daemon; a sixth, `audiocontrol-metadata`,
is the metadata code the `audiocontrol` binary links behind its default `metadata`
feature. `scripts/check-crate-deps.sh` enforces that the `audiocontrol` *library*
never depends on `audiocontrol-metadata` — `src/main.rs` is the one file that
links both.

| Crate | Responsibility | Key files |
|---|---|---|
| `acr-types` | Plain domain types and pure functions shared by both daemons: `Song`, `Artist`, `Album`, `Track`, `Identifier`, the enrichment payload types, the interface traits (`LibraryEnricher`, `Resolver`, `SongInformationSink`, `PlaybackStateSource`), and string/URL helpers. No Rocket, no I/O. | `song.rs`, `artist.rs`, `enrichment.rs`, `resolver.rs`, `now_playing.rs`, `urlprefix.rs` |
| `acr-http` | The outbound HTTP plumbing both daemons use: the retrying client, the per-service rate limiter. | `http_client.rs`, `retry.rs`, `ratelimit.rs` |
| `acr-images` | Image resizing and format handling shared by every cache that serves `?size=` variants: rung snapping, `@<size>` naming, format sniffing, grading. | `imageresize.rs`, `sniff.rs`, `image_grader.rs` |
| `acr-store` | The persistent stores each daemon initialises over its own directory: the SQLite attribute cache and settings DB, the image cache and its retired-rung purge, background jobs, genre cleanup. | `attributecache.rs`, `settingsdb.rs`, `imagecache.rs`, `imagepurge.rs`, `backgroundjobs.rs` |
| `acr-web` | The Rocket pieces both APIs share: the `ForwardedPrefix` guard, image responses with ETag/304, path validation, and the `/imagecache/<path..>` route factory each daemon mounts over its own cache. | `imageresponse.rs`, `validated.rs`, `imagecache.rs`, `urlprefix.rs` |
| `audiocontrol-metadata` | The metadata code: MusicBrainz/TheAudioDB/fanart.tv/Last.fm/Spotify clients, cover-art providers, the artist store, the library enricher and resolver the player daemon injects at startup, the security store, and the four CLI tools that only need this crate's code. | `musicbrainz.rs`, `lastfm.rs`, `spotify.rs`, `library_enricher.rs`, `security_store.rs`, `src/bin/*.rs` |

## The acr-webmcp bridge

`acr-webmcp` is not part of the Rust binary — it's a separate, dependency-free Python
HTTP server (`packages/acr-webmcp/src/acr-webmcp`) that translates MCP tool calls into
REST calls against ACR's own API and hands the JSON straight back. It holds no state of
its own.

| Tool group | Examples |
|---|---|
| Playback | `players_list`, `now_playing`, `playback_command` |
| Queue | `player_queue`, `queue_add_track`, `queue_play_index` |
| Library | `library_albums`, `library_albums_by_artist`, `library_categories` |
| Genre config | `genre_mapping_set`, `genre_ignore_add`, `genre_config_get` |

Full tool list: `docs/acr-webmcp.md` (repository root). No authentication is required on
the local network.

## Data flow traces

### Spotify track change (event flowing outward)

1. librespot's `--onevent` hook runs `audiocontrol_notify_librespot` with
   `PLAYER_EVENT=track_changed`.
2. The tool `POST`s the new track as JSON to `/api/player/librespot/update`.
3. Rocket routes it to `players::event_api::player_event_update`, which finds the
   controller named `librespot` and calls `process_api_event()`.
4. `players::librespot` updates its song, then calls
   `BaseController::notify_song_changed()`.
5. `EventBus` publishes `PlayerEvent::SongChanged` to every subscriber.
6. `ActiveMonitor` makes librespot the active player; WebSocket clients get the new
   track; the Last.fm plugin scrobbles it.

### "Pause the music" via Claude (command flowing inward)

1. Claude calls `playback_command` on `acr-webmcp` with
   `{player: "active", command: "pause"}`.
2. `acr-webmcp` `POST`s to `/api/player/active/command/pause` on ACR's REST API.
3. `send_command_to_player_by_name` resolves `"active"` via
   `AudioController::get_active_controller()` — say, `players::mpd`.
4. The string `"pause"` parses to `PlayerCommand::Pause` and is sent straight to that
   controller.
5. `players::mpd` issues MPD's own `pause` command over its TCP connection to the
   daemon.
6. MPD pauses; on its next status poll, `players::mpd` observes the change and
   publishes `StateChanged` back out.

## Deployment

| Component | Process | Address | Config / unit |
|---|---|---|---|
| ACR core | `/usr/bin/audiocontrol` | `0.0.0.0:1080` | `audiocontrol.service` · `/etc/audiocontrol/audiocontrol.json` · runs as user `audiocontrol` |
| acr-webmcp | `/usr/bin/acr-webmcp` | `127.0.0.1:13180` | `acr-webmcp.service` (user unit) · `ACR_API_BASE_URL` env var |
| Reverse proxy | nginx | `:80` | `/api/audiocontrol/*` → :1080, `/api/acr-webmcp/*` → :13180 |

| Path | Contents |
|---|---|
| `/etc/audiocontrol/audiocontrol.json` | Main config: `services`, `players`, `action_plugins`, `inputs`. |
| `/var/lib/audiocontrol/cache/attributes/cache.db` | SQLite attribute cache — metadata and lookups, in-memory-accelerated. |
| `/var/lib/audiocontrol/cache/images` | Cached cover art and images. |
| `/var/lib/audiocontrol/db/settings.db` | SQLite settings database — user configuration. |
| `auth.d/audiocontrol-auth.json` | AES-GCM encrypted secrets, managed by `SecurityStore`. |

## See also

- [API Documentation](api.md)
- [CLI Tools](cli_tools.md)
- [Generic Player Controller](generic_player_controller.md)
- [SystemD Integration](systemd_integration.md)
- `docs/acr-webmcp.md` (repository root)
