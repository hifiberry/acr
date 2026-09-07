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

/// Start the enrichment workers on `events`, or hand `events` back untouched.
///
/// `Ok(())` means something is reading them. **`Err(events)` returns the
/// receiver unconsumed, and the caller must decide what to do with it** --
/// because dropping it is no longer a harmless way of saying "nobody wants
/// these". The subscriber in [`crate::now_playing_ws`] ends its loop and closes
/// the socket at the first event it cannot deliver, and that socket now carries
/// `library_changed` for the library puller as well; dropping this receiver
/// therefore stops library enrichment being told anything, on an installation
/// whose only fault is having no now-playing worker configured.
///
/// This used to return a plain `bool` and drop the receiver on the way out,
/// which was right while now-playing was the socket's only purpose.
/// (`now_playing_bridge` on the player side had the same contract and
/// unsubscribed from the event bus instead, but the daemon no longer uses it.)
///
/// What has not changed is why an unread channel is refused rather than filled:
/// an unbounded channel nobody reads grows by one event per song change for the
/// life of the daemon.
pub fn start(
    events: Receiver<NowPlayingEvent>,
    sink: Arc<dyn SongInformationSink>,
    state: Arc<dyn PlaybackStateSource>,
    lastfm: Option<LastfmWorkerConfig>,
) -> Result<(), Receiver<NowPlayingEvent>> {
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
        debug!("No now-playing workers configured; not consuming events");
        return Err(events);
    }

    info!(
        "Now-playing enrichment started with {} worker(s)",
        senders.len()
    );
    fan_out(events, senders);
    Ok(())
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
    #[test]
    fn nothing_configured_means_the_events_are_handed_back_unconsumed() {
        // `configured_providers` reads a process-global installed from the
        // configuration, so say what this test needs rather than depending on
        // no other test in this process having installed one.
        crate::external_coverart::initialize_from_config(&serde_json::json!({}));

        let (tx, events) = unbounded();
        let sink = Arc::new(NullSink);
        let returned = start(events, sink.clone(), sink, None)
            .expect_err("nothing is configured, so nothing consumes the events");

        tx.send(state_event())
            .expect("the returned receiver keeps the channel open");
        assert_eq!(
            returned.recv_timeout(Duration::from_secs(1)).unwrap(),
            state_event(),
            "and it is the same channel, not a fresh one"
        );
    }
}
