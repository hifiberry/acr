# Shairport over MPRIS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ACR's shairport player reads and controls shairport-sync through MPRIS for AirPlay 1 and 2, including cover art, with the UDP path kept as a fallback.

**Architecture:** First the generic MPRIS controller is made dependable: it survives an absent player, publishes changes, derives capabilities, and imports local artwork into the image cache. Then the shairport controller wraps an MPRIS controller that shares its `BasePlayerController`, so name, id and listeners stay `shairport`. Pure functions (artwork resolution, snapshot reconciliation, command routing) carry the logic and the unit tests; D-Bus access stays thin.

**Tech Stack:** Rust 1.86, `dbus` 0.9 (blocking), `parking_lot`, `md5`, `urlencoding`, `tempfile` (dev), ACR's `imagecache` (`crates/acr-store`).

**Spec:** `doc/specs/2026-11-09-shairport-via-mpris.md`

## Global Constraints

- No new crates: use `md5`, `urlencoding`, `dbus`, `tempfile` already in `Cargo.toml`.
- Artwork URL form: `/api/imagecache/mpris/<md5 hex>.<ext>`, `<ext>` ∈ `jpg`, `png`, `gif`, `webp`; expiry 7 days.
- Maximum imported artwork size: `10 * 1024 * 1024` bytes.
- shairport MPRIS bus name: `org.mpris.MediaPlayer2.ShairportSync`, system bus.
- The shairport player's name and id stay `shairport`; `/etc/audiocontrol/players.d/shairport.json` must work unchanged.
- `metadata_source` values: `"mpris"` (default), `"udp"`.
- Never call back into `BasePlayerController` inside an `update_song` closure (non-reentrant lock; see its doc comment).
- Commit messages and docs in the project's voice; no tooling attribution.

## Review Focus

1. **shairport off at ACR start, switched on later in the WebUI** → the player appears within one poll interval, no ACR restart. (Task 4 test `poll_picks_up_player_that_appears_later` via `reconcile`, plus Task 7 check.)
2. **Artwork paths that must not be served:** `file://(null)`, a symlink in the cover directory pointing to `/etc/passwd`, a text file named `.jpg`, an 11 MiB file → no cover art, no file served. (Task 1 tests.)
3. **AirPlay session ends** → status `Stopped`; the last song must disappear, not linger. (Task 2 `stopped_clears_song_when_configured`.)
4. **Late artwork for the same song** → it reaches clients once, and a 1 s poll neither re-reads the file nor fires `song_changed` every second. (Task 1 memo test, Task 4 `late_artwork_merges_into_same_song`.)
5. **Another player takes over** → Stop must still drop the AirPlay session, not just pause it. (Task 5 `stop_drops_session`.)

---

### Task 1: Artwork resolver

**Files:**
- Create: `src/helpers/mpris_artwork.rs`
- Modify: `src/helpers/mod.rs` (add `pub mod mpris_artwork;`)
- Test: inline `#[cfg(test)] mod tests` in `src/helpers/mpris_artwork.rs`

**Interfaces:**
- Produces:
  - `pub const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;`
  - `pub trait ArtworkStore: Send + Sync { fn store(&self, key: &str, data: &[u8]) -> Result<String, String>; }` — `key` is `mpris/<md5>.<ext>`, returns the URL clients use.
  - `pub struct ImageCacheArtworkStore;` — `store` calls `imagecache::store_image_with_expiry(key, data, Some(now + 7 days))`, returns `format!("/api/imagecache/{key}")`.
  - `pub fn image_extension(data: &[u8]) -> Option<&'static str>` — magic bytes: `FF D8 FF` → `jpg`, `89 50 4E 47` → `png`, `GIF8` → `gif`, `RIFF` + `WEBP` at offset 8 → `webp`.
  - `pub struct ArtworkResolver` with `pub fn new(allowed_dirs: Vec<PathBuf>, store: Arc<dyn ArtworkStore>) -> Self` and `pub fn resolve(&self, art_url: &str) -> Option<String>`.

- [ ] **Step 1: Write the failing tests** (use a `RecordingStore` that counts calls and returns `/api/imagecache/{key}`; files in a `tempfile::tempdir()`):
  - `http_urls_pass_through`: `resolve("https://x/y.jpg") == Some("https://x/y.jpg")`, store not called.
  - `file_url_in_allowed_dir_is_imported`: JPEG bytes at `<dir>/cover.jpg`, `resolve("file://<dir>/cover.jpg") == Some(format!("/api/imagecache/mpris/{:x}.jpg", md5::compute(&bytes)))`.
  - `bare_path_is_imported`: same file, `resolve("<dir>/cover.jpg")` gives the same URL.
  - `percent_encoded_path_is_decoded`: file `cover a.png` (PNG bytes), `resolve("file://<dir>/cover%20a.png")` ends with `.png`.
  - `null_and_empty_give_none`: `resolve("file://(null)")` and `resolve("")` are `None`.
  - `file_outside_allowed_dirs_is_refused`: file in a second tempdir → `None`, store not called.
  - `symlink_escaping_allowed_dir_is_refused`: symlink `<dir>/evil.jpg` → JPEG file in another tempdir → `None`.
  - `non_image_is_refused`: `b"hello"` in `<dir>/x.jpg` → `None`.
  - `oversized_file_is_refused`: JPEG header + `MAX_ARTWORK_BYTES` bytes → `None`.
  - `unchanged_file_is_read_once`: two `resolve` calls on the same path → equal results, store called once; rewrite the file with different bytes → third call stores again with the new md5.

- [ ] **Step 2: Run tests to verify they fail**
  Run: `cargo test --lib mpris_artwork`
  Expected: FAIL (module does not exist).

- [ ] **Step 3: Implement `src/helpers/mpris_artwork.rs`**
  Strip `file://`, `urlencoding::decode`, require an absolute path, `std::fs::canonicalize` both the file and each allowed dir, and accept only if the file starts with a canonical allowed dir. Check `metadata().len() <= MAX_ARTWORK_BYTES` before reading. The memo is `Mutex<Option<(String /*raw url*/, u64 /*len*/, SystemTime /*mtime*/, Option<String> /*result*/)>>`, checked before any read. Log refusals at `debug!`.

- [ ] **Step 4: Run tests to verify they pass**
  Run: `cargo test --lib mpris_artwork`
  Expected: all 10 PASS.

- [ ] **Step 5: Commit**
  `git add src/helpers/mpris_artwork.rs src/helpers/mod.rs`
  `git commit -m "feat(mpris): import local artwork into the image cache"`

---

### Task 2: Snapshot reconciliation

**Files:**
- Create: `src/players/mpris/reconcile.rs`
- Modify: `src/players/mpris/mod.rs` (add `pub mod reconcile;`)
- Test: inline in `src/players/mpris/reconcile.rs`

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)] pub struct MprisFlags { pub can_control: bool, pub can_play: bool, pub can_pause: bool, pub can_go_next: bool, pub can_go_previous: bool, pub can_seek: bool, pub has_shuffle: bool, pub has_loop: bool }`
  - `#[derive(Debug, Clone, Default)] pub struct MprisSnapshot { pub status: Option<String>, pub shuffle: Option<bool>, pub loop_status: Option<String>, pub position_us: Option<i64>, pub flags: MprisFlags, pub song: Option<Song> }`
  - `pub fn capabilities_from_flags(flags: &MprisFlags) -> Vec<PlayerCapability>` — empty if `!can_control`; otherwise in this order: `Play` (can_play), `Pause` (can_pause), `Stop`, `Next` (can_go_next), `Previous` (can_go_previous), `Seek` (can_seek), `Position`, `Shuffle` (has_shuffle), `Loop` (has_loop).
  - `#[derive(Debug, Clone)] pub struct Reconciled { pub state: PlaybackState, pub shuffle: bool, pub loop_mode: LoopMode, pub position: Option<f64>, pub state_changed: bool, pub shuffle_changed: bool, pub loop_changed: bool, pub flags: MprisFlags, pub flags_changed: bool, pub song: Option<Song> }`
  - `pub fn reconcile(prev: &PlayerState, prev_flags: Option<MprisFlags>, snap: Option<&MprisSnapshot>, clear_song_when_stopped: bool) -> Reconciled` — `snap == None` means the player is not on the bus.

- [ ] **Step 1: Write the failing tests**
  - `absent_player_is_stopped_without_song`: prev `Playing`, `snap None` → `state == Stopped`, `state_changed`, `song.is_none()`, `flags == MprisFlags::default()`.
  - `status_strings_map`: `"Playing"`, `"Paused"`, `"Stopped"`, `"Bogus"` → `Playing`, `Paused`, `Stopped`, `Unknown`.
  - `shuffle_and_loop_changes_are_flagged`: prev shuffle false / loop `None`, snapshot `Some(true)` / `"Track"` → `shuffle_changed`, `loop_changed`, `loop_mode == Track`; unknown loop string → `LoopMode::None`.
  - `unchanged_snapshot_flags_nothing`: same values as prev → all `*_changed` false.
  - `stopped_clears_song_when_configured`: status `"Stopped"`, snapshot song `Some` → `song.is_none()` with `true`, `song.is_some()` with `false`.
  - `position_converts_microseconds`: `position_us = Some(2_500_000)` → `position == Some(2.5)`.
  - `flags_changed_only_on_difference`: same flags as `prev_flags` → `!flags_changed`; `prev_flags None` → `flags_changed`.
  - `shairport_flags_give_expected_capabilities`: flags control/play/pause/next/previous/shuffle/loop true, seek false → `[Play, Pause, Stop, Next, Previous, Position, Shuffle, Loop]`; `can_control false` → `[]`.
  - `poll_picks_up_player_that_appears_later`: prev `Stopped` from an absent poll, then a snapshot with `"Playing"` → `state_changed`, `state == Playing`, `flags_changed`.

- [ ] **Step 2: Run tests to verify they fail**
  Run: `cargo test --lib players::mpris::reconcile`
  Expected: FAIL (module does not exist).

- [ ] **Step 3: Implement `reconcile.rs`** as pure functions over the types above; no D-Bus.

- [ ] **Step 4: Run tests to verify they pass**
  Run: `cargo test --lib players::mpris::reconcile`
  Expected: all PASS.

- [ ] **Step 5: Commit**
  `git commit -m "feat(mpris): reconcile polled state into published changes"`

---

### Task 3: MPRIS configuration

**Files:**
- Create: `src/players/mpris/config.rs`
- Modify: `src/players/mpris/mod.rs` (add `pub mod config;`), `src/players/player_factory.rs:202-214` (the `"mpris"` arm)
- Test: inline in `src/players/mpris/config.rs`

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum BusPreference { Auto, System, Session }`
  - `#[derive(Debug, Clone)] pub struct MprisConfig { pub bus_name: String, pub name: Option<String>, pub bus: BusPreference, pub poll_interval: Duration, pub artwork_dirs: Vec<PathBuf>, pub clear_song_when_stopped: bool, pub extra_capabilities: Vec<PlayerCapability> }`
  - `impl MprisConfig { pub fn new(bus_name: &str) -> Self; pub fn from_json(v: &serde_json::Value) -> Result<Self, String> }` — defaults `Auto`, 1 s, `[]`, `false`, `[]`. JSON keys: `bus_name` (required), `name`, `bus` (`"auto"|"system"|"session"`), `poll_interval` (seconds, f64), `artwork_dirs` (array of strings), `clear_song_when_stopped` (bool). `extra_capabilities` is code-only.

- [ ] **Step 1: Write the failing tests**
  - `minimal_json_uses_defaults`: `{"bus_name":"org.mpris.MediaPlayer2.vlc"}` → `bus == Auto`, `poll_interval == 1s`, `name == None`, `artwork_dirs.is_empty()`, `!clear_song_when_stopped`.
  - `full_json_is_read`: all keys set → each field equals the JSON value.
  - `bad_bus_is_an_error`: `"bus":"dbus"` → `Err` whose text contains `"bus"`.
  - `missing_bus_name_is_an_error`: `{}` → `Err` containing `"bus_name"`.

- [ ] **Step 2: Run tests to verify they fail**
  Run: `cargo test --lib players::mpris::config` → FAIL.

- [ ] **Step 3: Implement `config.rs`; change the factory's `"mpris"` arm** to `MprisConfig::from_json(config_obj)` mapped to `PlayerCreationError::MissingField`/a config error, then `MprisPlayerController::with_config(cfg)` (added in Task 4; until then call `new_with_poll_interval(&cfg.bus_name, cfg.poll_interval)`).

- [ ] **Step 4: Run tests to verify they pass**
  Run: `cargo test --lib players::mpris` → PASS; `cargo build` succeeds.

- [ ] **Step 5: Commit**
  `git commit -m "feat(mpris): name, bus, artwork and stopped-song settings"`

---

### Task 4: Controller rework

**Files:**
- Modify: `src/players/mpris/mod.rs`, `src/helpers/mpris.rs` (add `pub fn read_snapshot(conn: &Connection, bus_name: &str) -> Option<MprisSnapshot>`), `src/players/player_factory.rs` (use `with_config`)
- Test: inline in `src/players/mpris/mod.rs`

**Interfaces:**
- Consumes: `ArtworkResolver`, `ImageCacheArtworkStore` (Task 1); `MprisSnapshot`, `MprisFlags`, `reconcile`, `capabilities_from_flags` (Task 2); `MprisConfig`, `BusPreference` (Task 3).
- Produces:
  - `pub fn with_config(config: MprisConfig) -> Self` — base named `config.name` or the last bus-name segment, id = bus name.
  - `pub fn with_config_and_base(config: MprisConfig, base: BasePlayerController) -> Self` — uses `base` as given (name/id untouched); Task 5 relies on this.
  - `pub fn apply_artwork(base: &BasePlayerController, resolver: &ArtworkResolver, song: Option<Song>)` — the song-storing step, testable without D-Bus.
  - `new` and `new_with_poll_interval` keep their signatures, built on `with_config`.

- [ ] **Step 1: Write the failing tests**
  - `late_artwork_merges_into_same_song`: base with a listener counting `song_changed`; `apply_artwork` with song A (no art), then song A with an `artUrl` in an allowed temp dir → stored song has the `/api/imagecache/mpris/…` URL, two notifications; a third identical call → still two.
  - `new_song_replaces_old`: song A then song B → stored song is B.
  - `with_config_and_base_keeps_name`: base `with_player_info("shairport","shairport")` → `get_player_name() == "shairport"`.
  - `default_capabilities_have_no_volume_or_kill`: `with_config(MprisConfig::new("x"))` → capabilities contain neither `Volume` nor `Killable`.

- [ ] **Step 2: Run tests to verify they fail**
  Run: `cargo test --lib players::mpris::tests` → FAIL.

- [ ] **Step 3: Implement**
  - `read_snapshot`: `PlaybackStatus`, `Shuffle`, `LoopStatus`, `Position`, `CanControl`, `CanPlay`, `CanPause`, `CanGoNext`, `CanGoPrevious`, `CanSeek`, `Metadata` → song via `extract_song_from_mpris_metadata`; `has_shuffle`/`has_loop` = property readable. `None` if the name is not on the bus.
  - `bus_type: Arc<RwLock<Option<BusType>>>`: `System`/`Session` fixed; `Auto` tries session then system on each poll while `None`, and resets to `None` when the player vanishes.
  - Poll step: `read_snapshot` → `reconcile(prev, prev_flags, snap, cfg.clear_song_when_stopped)` → write `current_state`; on `state_changed` `notify_state_changed`, on `shuffle_changed` `notify_random_changed`, on `loop_changed` `notify_loop_mode_changed`; on `flags_changed` `set_capabilities(capabilities_from_flags ∪ extra_capabilities, true)`; then `apply_artwork(&base, &resolver, r.song)`.
  - `apply_artwork`: resolve `cover_art_url` first (outside any lock), `base.set_song(resolved_song)`; if identity unchanged and the stored URL differs, `base.update_song(|s| …)` assigning the precomputed URL only.
  - `start()` always starts polling and returns `true`; logs `info!` when the player is not on the bus yet.
  - Initial capabilities: `capabilities_from_flags(&MprisFlags::default())` plus `extra_capabilities`.
  - `send_command`: unchanged mapping; `Seek` only when `Seek` is in the current capabilities.

- [ ] **Step 4: Run tests**
  Run: `cargo test --lib players::mpris && cargo clippy --all-targets -- -D warnings`
  Expected: PASS, no warnings.

- [ ] **Step 5: Commit**
  `git commit -m "feat(mpris): tolerate absent players, publish changes, derive capabilities"`

---

### Task 5: Shairport over MPRIS

**Files:**
- Modify: `src/players/shairport/shairport.rs`
- Test: inline (extend the existing `#[cfg(test)]` module)

**Interfaces:**
- Consumes: `MprisPlayerController::with_config_and_base`, `MprisConfig`, `BusPreference::System` (Tasks 3–4); existing `stop_airplay()`, `control_systemd_service("restart")`.
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MetadataSource { Mpris, Udp }`
  - `#[derive(Debug, Clone, PartialEq)] pub enum CommandRoute { Mpris(PlayerCommand), DropSession, RestartService, Unsupported }`
  - `pub fn route_command(cmd: &PlayerCommand, session_active: bool, has_systemd_unit: bool) -> CommandRoute`
  - `from_config` reads `metadata_source` (default `Mpris`; unknown value → `warn!` and `Mpris`).

- [ ] **Step 1: Write the failing tests**
  - `stop_drops_session`: `route_command(&Stop, true, true) == DropSession`.
  - `pause_next_previous_go_to_mpris`: `Pause`, `PlayPause`, `Next`, `Previous`, `SetRandom(true)`, `SetLoopMode(Track)` → `Mpris(same command)`.
  - `play_with_session_goes_to_mpris`: `route_command(&Play, true, true) == Mpris(Play)`.
  - `play_without_session_restarts_service`: `(&Play, false, true) == RestartService`; `(&Play, false, false) == Unsupported`.
  - `seek_is_unsupported`: `Seek(10.0)` → `Unsupported`.
  - `metadata_source_defaults_to_mpris`: `from_config(json!({"port":5555}))` → `Mpris`; `"udp"` → `Udp`; `"x"` → `Mpris`.

- [ ] **Step 2: Run tests to verify they fail**
  Run: `cargo test --lib players::shairport` → FAIL.

- [ ] **Step 3: Implement**
  In `Mpris` mode hold `mpris: Option<MprisPlayerController>` built with `with_config_and_base(cfg, self.base.clone())`, where `cfg` = bus name `org.mpris.MediaPlayer2.ShairportSync`, `BusPreference::System`, `artwork_dirs = [coverart_dir]`, `clear_song_when_stopped = true`, `extra_capabilities = [Metadata, AlbumArt]` plus `Play` when `systemd_unit` is set. `start`/`stop` start/stop the MPRIS controller instead of the UDP listener and watcher; `get_playback_state`, `get_song`, `get_position`, `get_shuffle`, `get_loop_mode` delegate. `send_command` uses `route_command` with `session_active = state != Stopped || song.is_some()`. `Udp` mode is today's code path, unchanged.

- [ ] **Step 4: Run tests**
  Run: `cargo test && cargo clippy --all-targets -- -D warnings` → PASS.

- [ ] **Step 5: Commit**
  `git commit -m "feat(shairport): read and control shairport-sync over MPRIS"`

---

### Task 6: Documentation and release

**Files:**
- Modify: `doc/mpris.md` (new keys table: `name`, `bus`, `poll_interval`, `artwork_dirs`, `clear_song_when_stopped`; an "Artwork from local files" section with the import rules; a "Shairport Sync" section with `metadata_source`)
- Modify: `doc/specs/2026-11-09-shairport-via-mpris.md` (`Status: Implemented`)
- Modify: `Cargo.toml` (both `version = "0.23.6"` lines → `0.24.0`), `debian/changelog` (new `0.24.0` entry, house format, crediting nobody but describing the change and the 5.6 AirPlay 2 gap it closes)

- [ ] **Step 1:** Write the docs; every key name and default must match `MprisConfig::from_json` and `MetadataSource`.
- [ ] **Step 2:** Bump the version; run `cargo build` and `dpkg-parsechangelog -S Version` → `0.24.0`.
- [ ] **Step 3: Commit** `git commit -m "docs: MPRIS settings, artwork import and shairport over MPRIS; release 0.24.0"`

---

### Task 7: Device verification

**Files:** none (checklist; results go into the PR description)

Build the package on the build host, install it on the test device, iPhone as the source. Run once with shairport-sync 5.5 (current package) and once with a 5.6.2 test build.

- [ ] AirPlay 2 and AirPlay 1 each: title, artist, album and cover art appear in the WebUI; position advances.
- [ ] Pause, play, next, previous from the WebUI act on the iPhone (AirPlay 1 both versions; AirPlay 2 on 5.6.2 only).
- [ ] Shuffle and repeat from the WebUI change on the iPhone (AirPlay 2, 5.6.2).
- [ ] Disconnect on the iPhone → the WebUI shows no song within 2 s.
- [ ] Switch AirPlay off in the WebUI, restart ACR, switch it on → the player appears without another ACR restart.
- [ ] Start another player while AirPlay plays → the AirPlay session is dropped.
- [ ] `journalctl -u audiocontrol` shows no `song_changed` storm (at most one per real change) during 5 minutes of playback.
- [ ] `metadata_source: "udp"` in `players.d/shairport.json` still works as before.
