//! One stream of now-playing events in, several workers out.
//!
//! Each worker gets its own channel, so a lookup that takes forty seconds
//! cannot hold up the scrobble timer, and neither can starve the other. A
//! worker that declines to start -- no cover art endpoint configured, Last.fm
//! disabled -- gets no channel at all: an unbounded channel nobody reads is a
//! leak that grows by one event per song change for as long as the daemon runs.

use acr_types::now_playing::{
    NowPlayingEvent, PlaybackStateSource, SongInformationSink, SplitterObservationSink,
};
use acr_types::{OrderResult, Song};
use crossbeam::channel::{unbounded, Receiver, Sender};
use log::{debug, info};
use std::sync::Arc;

use crate::lastfm_worker::LastfmWorkerConfig;

/// Start the enrichment workers on `events`. **Always consumes `events`.**
///
/// `true` means a worker is reading them; `false` means none was configured and
/// they are being drained and discarded. Either way this function, and not its
/// caller, is what keeps reading the channel -- because a caller that forgot to
/// would break something far away and silently.
///
/// The subscriber in [`crate::now_playing_ws`] ends its loop and closes the
/// socket at the first event it cannot deliver, and that socket also carries
/// `library_changed` for the library puller. So a dropped receiver stops
/// library enrichment being told anything at all, on an installation whose only
/// fault is having no now-playing worker configured -- and it does it after the
/// first song change, with a healthy-looking log.
///
/// That is why draining lives here rather than in the caller. An earlier
/// version returned the receiver for the caller to deal with; every test passed
/// with the caller dropping it, and the failure only appeared on a real daemon
/// with real song changes. A contract that has to be honoured by whoever calls
/// it is a contract that will one day not be.
///
/// What has not changed is why an unread channel is refused rather than left to
/// fill: an unbounded channel nobody reads grows by one event per song change
/// for the life of the daemon.
///
/// What has not changed is why an unread channel is refused rather than filled:
/// an unbounded channel nobody reads grows by one event per song change for the
/// life of the daemon.
pub fn start(
    events: Receiver<NowPlayingEvent>,
    sink: Arc<dyn SongInformationSink>,
    state: Arc<dyn PlaybackStateSource>,
    observations: Arc<dyn SplitterObservationSink>,
    lastfm: Option<LastfmWorkerConfig>,
) -> bool {
    let mut senders: Vec<Sender<NowPlayingEvent>> = Vec::new();

    let (cover_tx, cover_rx) = unbounded();
    if crate::external_coverart::worker::start(cover_rx, Arc::clone(&sink)) {
        senders.push(cover_tx);
    }

    if let Some(config) = lastfm {
        let (lastfm_tx, lastfm_rx) = unbounded();
        if crate::lastfm_worker::start(config, lastfm_rx, Arc::clone(&sink), state) {
            senders.push(lastfm_tx);
        }
    }

    // Only worth a channel and a thread when MusicBrainz is actually
    // reachable: `detect_order` already answers `Unknown` for everything
    // when it is disabled (`crate::title_order::detect_order` ->
    // `musicbrainz::search_recording` -> `is_enabled()`), which is not
    // actionable -- so an always-on worker would be a channel and a thread
    // that never had anything to report. Gating it here also keeps
    // `nothing_configured_still_leaves_the_channel_alive` below meaningful:
    // with MusicBrainz off too, "nothing configured" still means zero
    // workers, and the drain path stays reachable.
    if crate::musicbrainz::is_enabled() {
        let (order_tx, order_rx) = unbounded();
        start_title_order_correction(order_rx, observations);
        senders.push(order_tx);
    }

    if senders.is_empty() {
        debug!("No now-playing workers configured; draining events instead");
        drain_and_discard(events);
        return false;
    }

    info!(
        "Now-playing enrichment started with {} worker(s)",
        senders.len()
    );
    fan_out(events, senders);
    true
}

/// Read and throw away every event, so the channel stays open.
///
/// Not the same as dropping the receiver: dropping it disconnects the channel,
/// which ends the subscriber's loop and closes a socket that library enrichment
/// also depends on. Reading and discarding keeps the socket up for the events
/// this process does still care about.
fn drain_and_discard(events: Receiver<NowPlayingEvent>) {
    std::thread::Builder::new()
        .name("now-playing-discard".into())
        .spawn(move || {
            for _ in events {}
            debug!("Now-playing discard stopped: its event channel closed");
        })
        .expect("spawn now-playing discard");
}

/// Copy every event to every worker, on a thread of its own.
fn fan_out(events: Receiver<NowPlayingEvent>, senders: Vec<Sender<NowPlayingEvent>>) {
    std::thread::Builder::new()
        .name("now-playing-fanout".into())
        .spawn(move || {
            for event in events {
                for sender in &senders {
                    // A worker whose thread has gone away takes its channel
                    // with it. There is nothing to do about that here, and
                    // nothing worth logging on every song change either.
                    let _ = sender.send(event.clone());
                }
            }
            debug!("Now-playing fan-out stopped: its event channel closed");
        })
        .expect("spawn now-playing fan-out");
}

/// The station a title-order observation would be reported against, or
/// `None` when `song` has nothing that could plausibly be one.
///
/// MPD sets `stream_url` on every song it reports, radio and local library
/// track alike (`stream_url: Some(mpd_song.file.clone())` in
/// `src/players/mpd/mpd.rs`), but the splitter route's `<station>` is
/// documented as *the stream URL* -- and MPD only ever calls into the
/// splitter for a title with no artist tag at all, which a local file
/// almost always has. Reporting an observation for a local file's own path
/// would create a splitter entry, and spend a MusicBrainz lookup, for a
/// track nothing ever tried to split. Requiring a scheme (`"://"`) is what
/// tells a stream URL apart from a filesystem path here.
fn station_for(song: &Song) -> Option<&str> {
    let url = song.stream_url.as_deref()?;
    if url.contains("://") {
        Some(url)
    } else {
        None
    }
}

/// Whether a title-order observation is even worth asking MusicBrainz about:
/// both halves present and distinct. A song with no artist at all -- many
/// AirPlay sources never send one -- or a title identical to its artist has
/// nothing to teach the splitter and nothing to ask about.
fn correctable(song: &Song) -> Option<(&str, &str)> {
    let title = song.title.as_deref()?;
    let artist = song.artist.as_deref()?;
    if title.is_empty() || artist.is_empty() || title == artist {
        return None;
    }
    Some((artist, title))
}

/// Whether a MusicBrainz verdict is worth reporting as an observation.
///
/// Only `SongArtist` disagrees with what the player already assumed:
/// `detect_order(artist, title)` returning it means the player's `artist`
/// field is actually the song and its `title` field is actually the artist.
/// `ArtistSong` confirms the split the player already made -- nothing to
/// report -- and `Unknown`/`Undecided` decided nothing at all.
fn is_actionable(order: OrderResult) -> bool {
    order == OrderResult::SongArtist
}

/// Reports a wrong artist/title split as a per-station observation, on a
/// thread of its own.
///
/// This is what replaced asking the player daemon a network question on
/// every stream title change: `GET /resolve/title-order` is gone with the
/// one-way seam, and `SongTitleSplitter` on the player side now decides
/// locally and immediately, using a fixed heuristic for a title it has
/// neither been told about nor learned. What used to be answered before the
/// split happened is now corrected afterwards.
///
/// **Not through `song-information`.** That route identifies a song by
/// title and artist and refuses a partial that disagrees with either -- a
/// swap disagrees with both by construction, so it can never pass. The
/// correction travels instead through `POST
/// /player/<name>/splitter/<station>/observation`
/// (`SplitterObservationSink`), the same route a user sets a station's order
/// through by hand -- an order observation is per-station splitter state,
/// not information about one song. Like the cover art and Last.fm workers,
/// a MusicBrainz lookup can take a while, which is why this reads its own
/// channel on its own thread rather than running inline in `fan_out`.
///
/// **The currently playing song keeps its original, possibly wrong, split.**
/// Correcting it would need the song-information merge policy to accept a
/// re-identification, which is out of scope here -- see the module doc
/// comment on `SplitterObservationSink`. This is an accepted, bounded
/// regression against the resolver this replaces: most radio is "Artist -
/// Title", so the fallback the player used is right more often than not;
/// the resolver often answered `Unknown` anyway; and the *next* track from
/// the same station reads correctly once this observation is recorded.
/// Re-deriving the current song from its raw stream title once the splitter
/// has learned is possible later, and touches no merge policy -- just not
/// done here.
///
/// Returns the thread's `JoinHandle` so a test can wait for it to actually
/// finish processing -- by joining after closing its channel, not by
/// sleeping -- rather than for the daemon to do anything with it.
fn start_title_order_correction(
    events: Receiver<NowPlayingEvent>,
    observations: Arc<dyn SplitterObservationSink>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("title-order-correction".into())
        .spawn(move || {
            for event in events {
                let NowPlayingEvent::SongChanged { source, song: Some(song) } = event else {
                    continue;
                };
                let Some(station) = station_for(&song) else {
                    continue;
                };
                let Some((artist, title)) = correctable(&song) else {
                    continue;
                };
                let order = crate::title_order::detect_order(artist, title);
                if is_actionable(order.clone()) {
                    debug!(
                        "Title order observation for station {}: {:?}/{:?} looks swapped",
                        station, song.artist, song.title
                    );
                    observations.record_order_observation(&source.player_name, station, order);
                }
            }
            debug!("Title-order correction stopped: its event channel closed");
        })
        .expect("spawn title-order correction")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;
    use super::*;
    use acr_types::{PlaybackState, PlayerSource, Song};
    use std::time::Duration;

    struct NullSink;

    impl SongInformationSink for NullSink {
        fn apply(&self, _source: &PlayerSource, _partial: &Song) -> bool {
            false
        }
    }

    impl PlaybackStateSource for NullSink {
        fn playback_state(&self) -> PlaybackState {
            PlaybackState::Stopped
        }
    }

    impl SplitterObservationSink for NullSink {
        fn record_order_observation(&self, _player_name: &str, _station: &str, _order: OrderResult) -> bool {
            false
        }
    }

    fn state_event() -> NowPlayingEvent {
        NowPlayingEvent::StateChanged {
            source: PlayerSource::new("mpd".to_string(), "mpd".to_string()),
            state: PlaybackState::Playing,
        }
    }

    /// Every worker sees every event. A worker that only saw some of them would
    /// miss the song change its whole job hangs on.
    #[test]
    fn every_worker_receives_every_event() {
        let (tx, events) = unbounded();
        let (first_tx, first) = unbounded();
        let (second_tx, second) = unbounded();
        fan_out(events, vec![first_tx, second_tx]);

        tx.send(state_event()).expect("the fan-out should be reading");

        assert_eq!(
            first.recv_timeout(Duration::from_secs(5)).unwrap(),
            state_event()
        );
        assert_eq!(
            second.recv_timeout(Duration::from_secs(5)).unwrap(),
            state_event()
        );
    }

    /// One worker's channel going away must not stop the others: the cover art
    /// worker and the scrobbler have very different lifetimes.
    #[test]
    fn a_dead_worker_does_not_stop_the_others() {
        let (tx, events) = unbounded();
        let (dead_tx, dead) = unbounded::<NowPlayingEvent>();
        let (live_tx, live) = unbounded();
        drop(dead);
        fan_out(events, vec![dead_tx, live_tx]);

        tx.send(state_event()).expect("the fan-out should be reading");
        tx.send(state_event()).expect("the fan-out should be reading");

        for _ in 0..2 {
            assert_eq!(
                live.recv_timeout(Duration::from_secs(5)).unwrap(),
                state_event()
            );
        }
    }

    /// With no cover art endpoint configured and no Last.fm entry there is
    /// nothing to enrich with, and saying so is what keeps an unbounded channel
    /// nobody reads from growing for the life of the daemon.
    ///
    /// **The receiver comes back rather than being dropped**, which is the part
    /// that matters to the caller: the socket feeding it also carries
    /// `library_changed`, so whoever asked has to keep it open even with no
    /// now-playing worker to hand it to.
    /// With nothing configured, `start` drains the channel itself rather than
    /// handing it back. The property asserted is the one the subscriber checks:
    /// the sender is still connected afterwards, and stays connected once the
    /// queue has been consumed.
    ///
    /// It used to return the receiver for the caller to drain, and the caller
    /// dropping it broke library enrichment on a real daemon while every test
    /// stayed green. So the assertion is deliberately made through `start`'s own
    /// return, with no cooperation from this test beyond sending events.
    #[test]
    fn nothing_configured_still_leaves_the_channel_alive() {
        // `configured_providers` and `musicbrainz::is_enabled` both read a
        // process-global installed from the configuration, so say what this
        // test needs rather than depending on no other test in this process
        // having installed one.
        crate::external_coverart::initialize_from_config(&serde_json::json!({}));
        crate::musicbrainz::initialize_from_config(&serde_json::json!({}));

        let (tx, events) = unbounded();
        let sink = Arc::new(NullSink);
        assert!(
            !start(events, sink.clone(), sink.clone(), sink, None),
            "nothing is configured, so no worker is reading"
        );

        for _ in 0..3 {
            tx.send(state_event())
                .expect("start must keep the channel open, not drop the receiver");
        }

        // Drain, then send again: a dropped receiver disconnects the channel,
        // which is what ends the subscriber's loop and closes the socket that
        // library_changed also travels on.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !tx.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(tx.is_empty(), "the drain should have consumed the events");
        assert!(
            tx.send(state_event()).is_ok(),
            "and the channel is still connected afterwards"
        );
    }

    /// A radio-stream song: both halves present, and a `stream_url` that
    /// looks like one -- the shape `station_for` is looking for.
    fn song(artist: &str, title: &str) -> Song {
        Song {
            artist: Some(artist.to_string()),
            title: Some(title.to_string()),
            stream_url: Some("http://stream.example/radio".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn correctable_needs_both_halves_present_and_distinct() {
        assert!(correctable(&song("Artist", "Title")).is_some());
        assert!(correctable(&Song {
            artist: None,
            title: Some("Title".to_string()),
            ..Default::default()
        })
        .is_none());
        assert!(correctable(&Song {
            artist: Some("Artist".to_string()),
            title: None,
            ..Default::default()
        })
        .is_none());
        // Nothing to teach the splitter when both halves already agree.
        assert!(correctable(&song("Same", "Same")).is_none());
    }

    /// `station_for` is what stops a title-order observation -- and the
    /// MusicBrainz lookup that would precede it -- being sent for every
    /// ordinary library track MPD plays, not just radio streams. A local
    /// file's own path (no scheme) must not read as a station; a stream URL
    /// must; no `stream_url` at all (every other backend) must not either.
    #[test]
    fn station_for_needs_a_url_shaped_stream_url() {
        assert_eq!(
            station_for(&song("Artist", "Title")),
            Some("http://stream.example/radio")
        );
        assert_eq!(
            station_for(&Song {
                stream_url: Some("Music/Artist/Album/Track.flac".to_string()),
                ..song("Artist", "Title")
            }),
            None,
            "a local file's own path is not a splitter station"
        );
        assert_eq!(
            station_for(&Song {
                stream_url: None,
                ..song("Artist", "Title")
            }),
            None
        );
    }

    /// The one actionable verdict: `SongArtist` means the player's `artist`
    /// field is actually the song and its `title` field is actually the
    /// artist. `ArtistSong` confirms the existing split, and `Unknown`/
    /// `Undecided` decided nothing -- none of the three is worth reporting.
    #[test]
    fn only_song_artist_is_actionable() {
        assert!(is_actionable(OrderResult::SongArtist));
        assert!(!is_actionable(OrderResult::ArtistSong));
        assert!(!is_actionable(OrderResult::Unknown));
        assert!(!is_actionable(OrderResult::Undecided));
    }

    /// A stub that records every observation it is handed, so a test can
    /// assert on what the worker actually reported rather than trusting that
    /// it ran.
    struct RecordingObservationSink(std::sync::Mutex<Vec<(String, String, OrderResult)>>);

    impl SplitterObservationSink for RecordingObservationSink {
        fn record_order_observation(&self, player_name: &str, station: &str, order: OrderResult) -> bool {
            self.0
                .lock()
                .unwrap()
                .push((player_name.to_string(), station.to_string(), order));
            true
        }
    }

    /// With MusicBrainz disabled -- the default in this process, and every
    /// build that has not configured it -- `detect_order` answers `Unknown`
    /// for everything, which `is_actionable` rejects, so the correction
    /// worker must never call the sink at all. This is the one live-wiring
    /// behaviour of the worker this test suite can assert without reaching
    /// MusicBrainz over the network: it reads its events and stays quiet
    /// rather than hanging or panicking.
    #[test]
    fn with_musicbrainz_disabled_the_worker_sends_no_observation() {
        crate::musicbrainz::initialize_from_config(&serde_json::json!({}));

        let (tx, events) = unbounded();
        let sink = Arc::new(RecordingObservationSink(std::sync::Mutex::new(Vec::new())));
        let handle = start_title_order_correction(events, sink.clone());

        tx.send(NowPlayingEvent::SongChanged {
            source: PlayerSource::new("mpd".to_string(), "mpd".to_string()),
            song: Some(song("Hey Jude", "The Beatles")),
        })
        .expect("the worker should be reading");

        // Closing the channel and joining the thread waits for it to have
        // actually finished processing the event, rather than sleeping and
        // hoping: with MusicBrainz off, `detect_order` cannot have answered
        // `SongArtist`, so nothing should have reached the sink.
        drop(tx);
        handle.join().expect("the worker thread should not panic");
        assert!(sink.0.lock().unwrap().is_empty());
    }
}
