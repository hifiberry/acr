//! One stream of now-playing events in, several workers out.
//!
//! Each worker gets its own channel, so a lookup that takes forty seconds
//! cannot hold up the scrobble timer, and neither can starve the other. A
//! worker that declines to start -- no cover art endpoint configured, Last.fm
//! disabled -- gets no channel at all: an unbounded channel nobody reads is a
//! leak that grows by one event per song change for as long as the daemon runs.

use acr_types::now_playing::{NowPlayingEvent, PlaybackStateSource, SongInformationSink};
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
    lastfm: Option<LastfmWorkerConfig>,
) -> bool {
    let mut senders: Vec<Sender<NowPlayingEvent>> = Vec::new();

    let (cover_tx, cover_rx) = unbounded();
    if crate::external_coverart::worker::start(cover_rx, Arc::clone(&sink)) {
        senders.push(cover_tx);
    }

    if let Some(config) = lastfm {
        let (lastfm_tx, lastfm_rx) = unbounded();
        if crate::lastfm_worker::start(config, lastfm_rx, sink, state) {
            senders.push(lastfm_tx);
        }
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
        // `configured_providers` reads a process-global installed from the
        // configuration, so say what this test needs rather than depending on
        // no other test in this process having installed one.
        crate::external_coverart::initialize_from_config(&serde_json::json!({}));

        let (tx, events) = unbounded();
        let sink = Arc::new(NullSink);
        assert!(
            !start(events, sink.clone(), sink, None),
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
}
