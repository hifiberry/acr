//! Forwards SongChanged and StateChanged from the EventBus into a channel of
//! NowPlayingEvent, and answers the metadata side's two questions about the
//! player through the controller. This is the whole of what the player side
//! knows about enrichment: no worker here reaches into the bus or the
//! controller, and nothing here knows what enrichment does.

use crate::audiocontrol::eventbus::{EventBus, EventSubscription};
use crate::audiocontrol::AudioController;
use crate::data::PlayerEvent;
use crate::players::PlayerController;
use acr_types::now_playing::{NowPlayingEvent, PlaybackStateSource, SongInformationSink};
use acr_types::{PlaybackState, PlayerSource, Song};
use crossbeam::channel::{unbounded, Receiver};
use log::debug;
use std::sync::Arc;

/// Subscribe to the two event kinds enrichment cares about and forward them.
///
/// Dropping the returned receiver is how a caller says it wants nothing: the
/// forwarding thread notices the closed channel, unsubscribes from the bus and
/// exits, so neither the channel nor the bus's subscriber map grows for events
/// no one will read.
pub fn start(bus: &EventBus) -> Receiver<NowPlayingEvent> {
    let (tx, rx) = unbounded();
    let (id, events) = bus.subscribe(vec![
        EventSubscription::SongChanged,
        EventSubscription::StateChanged,
    ]);
    let bus = bus.clone();

    std::thread::Builder::new()
        .name("now-playing-bridge".into())
        .spawn(move || {
            for event in events {
                let forwarded = match event {
                    PlayerEvent::SongChanged { source, song } => {
                        NowPlayingEvent::SongChanged { source, song }
                    }
                    PlayerEvent::StateChanged { source, state } => {
                        NowPlayingEvent::StateChanged { source, state }
                    }
                    // The subscription asks for nothing else, so this is only
                    // reached if the bus ever widens what it delivers.
                    _ => continue,
                };
                if tx.send(forwarded).is_err() {
                    debug!("Now-playing bridge: nothing is listening, unsubscribing");
                    break;
                }
            }
            bus.unsubscribe(id);
        })
        .expect("spawn now-playing bridge");

    rx
}

/// The player side of the enrichment seam.
///
/// `apply` delegates to `AudioController::apply_song_information` and adds
/// nothing: the partial-merge rules -- cover art replacing only a placeholder,
/// `liked`, the `metadata` map, the `cover_art_source` provenance and dropping
/// an answer that no longer describes the song being played -- are that
/// method's contract, and a second copy of them here would be a second
/// contract to keep in step.
pub struct ControllerSink(pub Arc<AudioController>);

impl SongInformationSink for ControllerSink {
    fn apply(&self, source: &PlayerSource, partial: &Song) -> bool {
        self.0.apply_song_information(source, partial)
    }
}

impl PlaybackStateSource for ControllerSink {
    fn playback_state(&self) -> PlaybackState {
        self.0.get_playback_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn source() -> PlayerSource {
        PlayerSource::new("mpd".to_string(), "mpd".to_string())
    }

    /// The two kinds enrichment acts on arrive; the rest of the bus does not.
    /// A position event every second would otherwise wake every worker for
    /// nothing.
    #[test]
    fn song_and_state_events_are_forwarded_and_others_dropped() {
        let bus = EventBus::new();
        let rx = start(&bus);
        let source = source();

        bus.publish(PlayerEvent::PositionChanged {
            source: source.clone(),
            position: 1.0,
        });
        bus.publish(PlayerEvent::StateChanged {
            source: source.clone(),
            state: PlaybackState::Playing,
        });

        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the state change should be forwarded");
        assert_eq!(
            got,
            NowPlayingEvent::StateChanged {
                source: source.clone(),
                state: PlaybackState::Playing,
            },
            "the position event must not appear before the state change"
        );
    }

    #[test]
    fn a_song_change_carries_the_song_and_its_source() {
        let bus = EventBus::new();
        let rx = start(&bus);
        let source = source();
        let song = Song {
            title: Some("Uni Acronym".to_string()),
            artist: Some("Alva Noto".to_string()),
            ..Default::default()
        };

        bus.publish(PlayerEvent::SongChanged {
            source: source.clone(),
            song: Some(song.clone()),
        });

        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the song change should be forwarded");
        assert_eq!(
            got,
            NowPlayingEvent::SongChanged {
                source,
                song: Some(song),
            }
        );
    }

    /// A song change to None is what a stopping player reports, and the
    /// Last.fm worker clears its track data on it, so it has to survive the
    /// crossing.
    #[test]
    fn a_song_change_to_none_is_forwarded() {
        let bus = EventBus::new();
        let rx = start(&bus);
        let source = source();

        bus.publish(PlayerEvent::SongChanged {
            source: source.clone(),
            song: None,
        });

        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the song change should be forwarded");
        assert_eq!(got, NowPlayingEvent::SongChanged { source, song: None });
    }

    /// Dropping the receiver has to end the subscription. Without it, a daemon
    /// with no enrichment configured would grow the channel by one event per
    /// song change for as long as it runs.
    #[test]
    fn dropping_the_receiver_unsubscribes_from_the_bus() {
        let bus = EventBus::new();
        let rx = start(&bus);
        drop(rx);

        // The bridge only notices on its next send, so publish until the
        // subscriber is gone rather than assuming a timing.
        for _ in 0..50 {
            bus.publish(PlayerEvent::StateChanged {
                source: source(),
                state: PlaybackState::Playing,
            });
            if bus.subscriber_count() == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the bridge should have unsubscribed once its receiver was dropped");
    }
}
