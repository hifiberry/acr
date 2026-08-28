# Emitting shuffle, loop and position events from the MPD controller

**Date:** 2026-08-28
**Status:** Proposed, not implemented
**Affects:** `src/players/mpd/mpd.rs`, the WebSocket event contract, every API client

## Problem

The WebSocket event vocabulary is **conditional on which player is active**, and
nothing in the API says so.

`notify_random_changed` and `notify_loop_mode_changed` are called by the LMS,
librespot, generic and raat controllers. The MPD controller calls neither. It
also never calls `notify_position_changed`. So on MPD — the default player on
most installs — toggling shuffle, cycling the loop mode, or seeking produces no
WebSocket event at all.

A client cannot discover this. It subscribes, waits, and nothing arrives. The
WebUI carried `// ! We don't get 'loop_mode_changed', nothing getting` comments
for exactly this reason; they were accurate field observations that outlived
several attempts to explain them away.

### Evidence

Captured from a 0.9.0 device while a person toggled shuffle, cycled the loop
mode, seeked, and paused and resumed on the MPD player. Over 120 seconds the
socket received:

```
5 state_changed
1 welcome
1 subscription_updated
```

No `random_changed`, no `loop_mode_changed`, no `position_changed`,
no `song_changed`. The capture is held as a fixture in the `hbos-contract`
work as negative evidence.

### What MPD emits today

| Event | Emitted by MPD controller? |
|---|---|
| `state_changed` | yes |
| `song_changed` | yes |
| `queue_changed` | yes |
| `capabilities_changed` | yes |
| `database_updating` | yes |
| `shuffle_changed` (`notify_random_changed`) | **no** |
| `loop_mode_changed` | **no** |
| `position_changed` | **no** |

## The change is smaller than it looks

MPD is **not** polled. `process_events` in `src/players/mpd/mpd.rs` sits in MPD's
`idle` loop and already subscribes to:

```rust
Subsystem::Player, Subsystem::Mixer, Subsystem::Options,
Subsystem::Playlist, Subsystem::Database, Subsystem::Update,
```

`Subsystem::Options` is precisely MPD's subsystem for `random`, `repeat`,
`single` and `consume`. **The event already arrives.** Its handler is a stub:

```rust
Subsystem::Options => {
    warn!("Options changed (repeat, random, etc.)");
    // Could query and notify about repeat/random state
},
```

So shuffle and loop need no new transport, no polling, and no new connection —
only a handler body. That stub comment has been describing the fix for as long
as it has existed.

## Scope

**In scope:** `shuffle_changed` and `loop_mode_changed` from `Subsystem::Options`.

**Separate, deliberately:** `position_changed`. MPD's idle protocol has no
subsystem that fires as position advances, because position advances
continuously. Emitting it is a different design with a different cost, treated
in its own section below.

**Out of scope:** changing the wire format, renaming events, or altering what
other player backends emit.

## Design

### Mapping MPD state to the event payloads

MPD status carries `random: bool`, `repeat: bool`, `single: bool`. The mapping
to `LoopMode`:

| `repeat` | `single` | `LoopMode` | emitted `mode` |
|---|---|---|---|
| false | any | `LoopMode::None` | `"no"` |
| true | true | `LoopMode::Track` | `"song"` |
| true | false | `LoopMode::Playlist` | `"playlist"` |

`random` maps directly to `notify_random_changed(enabled)`.

Note the asymmetry that already exists elsewhere in the API and must not be
"fixed" here: the emitted vocabulary is `no | song | playlist`, while the
inbound `set_loop:<mode>` command parses `none | track | playlist`. This spec
changes neither.

### Diffing is required, not optional

`Subsystem::Options` fires for **any** option change — including `consume`,
`crossfade` and replay-gain settings that carry no event of their own. Emitting
unconditionally on every `Options` event would produce a `shuffle_changed`
storm whenever an unrelated option moved, and clients that refetch on any event
would then hammer the REST API.

The controller must therefore hold a last-known snapshot of `(random, repeat,
single)` and emit only on an actual transition:

- On the **first** observation after connect, record the baseline and emit
  nothing. A reconnect must not look like the user toggling something.
- On subsequent observations, emit `shuffle_changed` only if `random` changed,
  and `loop_mode_changed` only if the derived `LoopMode` changed. Both may fire
  from one `Options` event; that is correct.

### Concurrency constraints

`.codereview.toml` in this repository names the hazards this touches, and they
apply directly:

- **Never hold the snapshot lock across a `notify_*` call.** Read the previous
  values, release the lock, compare, then notify. The notify path reaches the
  event bus and every subscribed WebSocket client; holding a player lock across
  it invites the lock-ordering deadlock the review guidance calls out.
- **No `unwrap()` or `expect()` on this path.** It is reachable from a player
  event, so a panic takes down audio control for the whole device. A failure to
  obtain a client or read status is logged and skipped; the next `Options` event
  re-synchronises, and the snapshot is left unchanged so no transition is lost.

### Logging

`warn!` is the wrong level for a routine user action. The `Options` and
`Playlist` stubs both use it, which makes normal operation look like a fault in
logs. Emitting handlers should log at `debug!`.

## `position_changed`, treated separately

MPD cannot push position. Two options:

1. **Reuse `PlayerProgress`** (`src/helpers/playback_progress.rs`), already used
   by the librespot and generic controllers. It holds a position, an
   `is_playing` flag and a `last_update` instant, and interpolates between
   updates. The MPD controller would seed it from `status` on every `Player`
   event — which already fires on play, pause, seek and track change — and let
   clients interpolate rather than receiving a stream of position events. This
   adds no periodic traffic at all.
2. **Emit periodically** on a timer. Simple for clients, but every connected
   client pays for it continuously, and the interval is a guess.

**Recommendation: (1).** It matches what the other controllers already do, costs
nothing when nothing changes, and the seek case — the one that actually needs an
event — is already covered by the `Player` subsystem firing.

## Discoverability

Fixing MPD closes today's gap but not the general problem: a client still cannot
tell which events a given player emits, and version-sniffing audiocontrol does
not answer it either, because the answer varies per player within one version.

The player object already carries `supports_api_events` (currently `false` for
MPD). Either give that field a documented meaning covering emitted events, or
add an explicit per-player list of emitted event types to the player payload.
Then a client degrades from data instead of a hardcoded list of player names,
and the next backend to lack an event does not cost another afternoon of
packet-watching.

This is the part worth getting right. The MPD fix is a day's work; the
discoverability contract is what stops the next client rediscovering this the
way the last two did.

## Compatibility

Additive for clients that already subscribe to `shuffle_changed` and
`loop_mode_changed` — they simply begin receiving them from MPD. No client
breaks by receiving an event it asked for.

One consequence worth stating: the WebUI's post-command refetch in `player.ts`
is currently **load-bearing** on MPD, not a legacy safety net, because no event
follows a shuffle or loop command there. It must not be removed on the strength
of this change alone, since devices running older audiocontrol will still emit
nothing.

## Testing

- Unit-test the `(repeat, single) -> LoopMode` mapping across all four
  combinations, including `repeat=false, single=true`.
- Unit-test the diffing: no emission on first observation; emission only on an
  actual transition; an `Options` event whose random/repeat/single are unchanged
  emits nothing.
- Against a real device, record a WebSocket capture while toggling shuffle and
  cycling loop, and confirm `shuffle_changed` and `loop_mode_changed` arrive with
  the expected payloads — the same procedure that produced the negative evidence
  above, which should now come back positive.
