# Shairport over MPRIS

**Date:** 2026-11-09
**Status:** Proposed
**Affects:** `src/players/mpris/*`, `src/helpers/mpris.rs`, a new
`src/helpers/mpris_artwork.rs`, `src/players/shairport/shairport.rs`,
`src/players/player_factory.rs`, `doc/mpris.md`

## Why

The shairport player learns what is playing from shairport-sync's UDP metadata
socket (`127.0.0.1:5555`) plus a watcher on its cover art directory. That path
has three problems:

1. **It goes blind on AirPlay 2 from shairport-sync 5.6.** 5.6 makes
   `get_plist_metadata = "yes"` the default, and the plist handler
   (`metadata_hub_handle_command_plist.c`) forwards title, album and artwork to
   the pipe and MQTT only — never to the multicast/UDP queue. AirPlay 1 is
   unaffected.
2. **It cannot control AirPlay 2.** Pause and Stop drop the whole session; there
   is no next/previous and no real pause.
3. **It has no position, shuffle or repeat.**

shairport-sync's MPRIS interface (`org.mpris.MediaPlayer2.ShairportSync`, on
the system bus) covers all of this for both AirPlay versions. Both versions
fill one metadata store, so MPRIS behaves the same for each:

| | AirPlay 1 | AirPlay 2 |
|---|---|---|
| title, artist, album, genre, track number, length, artwork, track id | ✓ | ✓ |
| `PlaybackStatus`, `Position` (computed live on read) | ✓ | ✓ |
| Play/Pause/PlayPause/Next/Previous/Stop | ✓ (DACP) | ✓ (5.6+; 5.5 has none) |
| `Shuffle`, `LoopStatus`, readable and writable | – | ✓ (5.6+) |
| Seek | ✗ (`CanSeek` false) | ✗ |

Its D-Bus policy lets any user call it; the `audiocontrol` user can read it
(checked on a device running 5.5).

## What changes

### 1. The generic MPRIS player becomes robust enough to rely on

- **Player present or not.** `start()` succeeds whether or not the player is on
  the bus, and polling keeps going. A player that is absent reads as
  `Stopped` with no song; one that appears later is picked up without
  restarting ACR. The bus is found per poll while unknown (`bus: "auto"`), or
  fixed by config (`"system"`/`"session"`).
- **Changes are published.** Playback state, shuffle and loop mode changes
  reach listeners (`notify_state_changed`, `notify_random_changed`,
  `notify_loop_mode_changed`), not just the internal state.
- **Capabilities follow the player.** They are derived from `CanControl`,
  `CanPlay`, `CanPause`, `CanGoNext`, `CanGoPrevious`, `CanSeek`, and from
  whether `Shuffle`/`LoopStatus` exist, and are re-published when they change.
  `Volume` is no longer advertised: nothing implements it.
- **Configurable identity.** Optional `name` overrides the name derived from
  the bus name.
- **Stopped means no song** when `clear_song_when_stopped` is set, because
  shairport-sync keeps the last song in `Metadata` after a session ends.

### 2. Artwork from local files becomes reachable

An `mpris:artUrl` that is a local file — `file:///…` or a bare absolute path,
as shairport-sync 5.6 sends — is useless to a browser. It is imported into the
image cache and replaced with `/api/imagecache/mpris/<md5>.<ext>`, the same way
the shairport player already serves its covers. Rules:

- `http://`/`https://` URLs pass through unchanged, as today.
- Local files are imported only from directories listed in `artwork_dirs`
  (default: none), after resolving symlinks, so a process on the bus cannot get
  an arbitrary file served over HTTP.
- Only real images are imported: JPEG, PNG, GIF or WebP by magic bytes, at most
  10 MiB.
- `file://(null)` (shairport-sync 5.5 with no artwork), empty, unreadable or
  rejected → no cover art.
- The result is remembered per (path, size, mtime), so polling every second
  does not re-read the file.
- Artwork that arrives after the title is merged with `update_song`, not
  dropped by `set_song`'s identity check.

### 3. The shairport player reads MPRIS by default

New key `metadata_source`: `"mpris"` (default) or `"udp"` (today's code,
unchanged, as a fallback). In MPRIS mode the player keeps its name and id
(`shairport`, which the WebUI and descriptors use) and its `systemd_unit`, and
wraps an MPRIS controller on the system bus with `artwork_dirs =
[coverart_dir]` and `clear_song_when_stopped = true`. Commands:

| Command | MPRIS mode |
|---|---|
| Pause, PlayPause, Next, Previous, SetRandom, SetLoopMode | MPRIS method / property |
| Stop | D-Bus `DropSession`, as today (another player is taking over) |
| Play | MPRIS `Play` while a session exists; with no session, restart the systemd unit as today |
| Seek | refused |

No packaging change: `/etc/audiocontrol/players.d/shairport.json` needs no new
key, and the change works with shairport-sync 5.5 and 5.6. With it in place,
hifiberry-shairport needs no multicast patch for 5.6.

## Out of scope

Volume over MPRIS (ACR controls volume at the mixer); D-Bus
`PropertiesChanged` signals instead of polling; deleting the UDP code (a later
release, once MPRIS has shipped).
