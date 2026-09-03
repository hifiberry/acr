//! The background lookup that keeps slow endpoints off every request path.
//!
//! A lookup here may take tens of seconds, so it runs on its own thread and
//! reports its answer the way every other late enrichment does: as a partial
//! song through `AudioController::apply_song_information`, which merges it,
//! stamps provenance, drops it if the song has moved on, and publishes
//! `song_information_update`.

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;

use log::{debug, info, warn};
use parking_lot::Mutex;

use crate::audiocontrol::eventbus::{EventBus, EventSubscription};
use crate::audiocontrol::AudioController;
use crate::data::song::COVER_ART_SOURCE;
use crate::data::{PlayerEvent, PlayerSource, Song};

use super::config::Trigger;
use super::{cache_key, configured_providers, ExternalCoverartProvider};
// `provider.name()` is a CoverartProvider method, so the trait must be in
// scope even though nothing here names it directly.
use crate::helpers::coverart::{CoverartProvider, CoverartQuery};

/// Whether this song is worth spending a slow lookup on.
///
/// Under `Fallback` the test is the song's own replaceability marker, the
/// same one `apply_song_information` enforces with. That keeps the two in
/// step: a song whose artwork would be refused is never paid for.
pub fn should_look_up(song: &Song, trigger: Trigger) -> bool {
    // A lookup keyed on nothing cannot be cached or matched back to the song.
    if song.title.is_none() || song.artist.is_none() {
        return false;
    }

    match trigger {
        Trigger::Always => true,
        Trigger::Fallback => song.cover_art_is_replaceable(),
    }
}

/// The partial to hand back for an answer.
///
/// `title` and `artist` are for identification, not update:
/// `apply_song_information` compares them and drops an answer that no longer
/// describes the song being played, which a 40-second lookup often does not.
/// Naming the source keeps a replaced placeholder's marker from being left
/// standing over the new image.
pub fn partial_for(song: &Song, provider_name: &str, url: &str) -> Song {
    let mut partial = Song {
        title: song.title.clone(),
        artist: song.artist.clone(),
        cover_art_url: Some(url.to_string()),
        ..Default::default()
    };
    partial.metadata.insert(
        COVER_ART_SOURCE.to_string(),
        serde_json::Value::String(provider_name.to_string()),
    );
    partial
}

/// Look one song up against one endpoint and report what comes back.
fn run_lookup(provider: &Arc<ExternalCoverartProvider>, song: &Song, source: &PlayerSource) {
    let (Some(title), Some(artist)) = (song.title.clone(), song.artist.clone()) else {
        return;
    };

    let query = CoverartQuery::Song { title, artist };
    let urls = provider.lookup(&query).urls();

    let Some(url) = urls.into_iter().next() else {
        debug!(
            "External cover art '{}': no artwork for {:?}",
            provider.name(),
            song.title
        );
        return;
    };

    let partial = partial_for(song, provider.name(), &url);
    let applied = AudioController::instance().apply_song_information(source, &partial);
    if applied {
        info!(
            "External cover art '{}': artwork applied for {:?}",
            provider.name(),
            song.title
        );
    } else {
        // Ordinary: the song changed while the lookup was in flight, or the
        // player supplied its own artwork in the meantime.
        debug!(
            "External cover art '{}': answer for {:?} no longer applies",
            provider.name(),
            song.title
        );
    }
}

/// Start one listener thread. Does nothing when no endpoint is configured.
pub fn start() {
    let providers = configured_providers();
    if providers.is_empty() {
        return;
    }

    // Cache keys currently being looked up. Two players showing the same
    // track, or a song changing back and forth, must not buy the same
    // 40-second answer twice.
    let in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    let (_id, receiver) = EventBus::instance().subscribe(vec![EventSubscription::SongChanged]);

    thread::spawn(move || {
        info!(
            "External cover art worker started for {} endpoint(s)",
            providers.len()
        );

        for event in receiver.iter() {
            let PlayerEvent::SongChanged { song: Some(song), source, .. } = event else {
                continue;
            };

            for provider in &providers {
                if !should_look_up(&song, provider.endpoint().trigger) {
                    continue;
                }

                let (Some(title), Some(artist)) = (song.title.clone(), song.artist.clone()) else {
                    continue;
                };
                let key = cache_key(
                    provider.name(),
                    &CoverartQuery::Song { title, artist },
                );

                if !in_flight.lock().insert(key.clone()) {
                    debug!("External cover art '{}': {} already in flight", provider.name(), key);
                    continue;
                }

                let provider = provider.clone();
                let song = song.clone();
                let source = source.clone();
                let in_flight = in_flight.clone();
                thread::spawn(move || {
                    run_lookup(&provider, &song, &source);
                    in_flight.lock().remove(&key);
                });
            }
        }

        warn!("External cover art worker stopped: the event bus closed");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::song::{
        COVER_ART_SOURCE, COVER_ART_SOURCE_LASTFM, COVER_ART_SOURCE_STATION_LOGO,
    };

    fn song_with(cover: Option<&str>, source: Option<&str>) -> Song {
        let mut song = Song {
            title: Some("Uni Acronym".to_string()),
            artist: Some("Alva Noto".to_string()),
            cover_art_url: cover.map(str::to_string),
            ..Default::default()
        };
        if let Some(source) = source {
            song.metadata.insert(
                COVER_ART_SOURCE.to_string(),
                serde_json::Value::String(source.to_string()),
            );
        }
        song
    }

    /// The case the feature exists for.
    #[test]
    fn fallback_looks_up_a_song_with_no_artwork() {
        assert!(should_look_up(&song_with(None, None), Trigger::Fallback));
    }

    /// A radio station's logo is a placeholder, and replacing it is the
    /// documented purpose of song_information_update.
    #[test]
    fn fallback_looks_up_a_song_showing_only_a_station_logo() {
        let song = song_with(
            Some("https://radio.example/logo.png"),
            Some(COVER_ART_SOURCE_STATION_LOGO),
        );
        assert!(should_look_up(&song, Trigger::Fallback));
    }

    /// Artwork the player supplied is the track's own and is never replaced,
    /// so paying for a lookup would buy an answer that gets discarded.
    #[test]
    fn fallback_skips_a_song_that_already_has_its_own_artwork() {
        let song = song_with(Some("https://player.example/cover.jpg"), None);
        assert!(!should_look_up(&song, Trigger::Fallback));
    }

    #[test]
    fn fallback_skips_a_song_whose_artwork_another_lookup_already_found() {
        let song = song_with(
            Some("https://lastfm.example/cover.jpg"),
            Some(COVER_ART_SOURCE_LASTFM),
        );
        assert!(!should_look_up(&song, Trigger::Fallback));
    }

    #[test]
    fn always_looks_up_regardless_of_existing_artwork() {
        let song = song_with(Some("https://player.example/cover.jpg"), None);
        assert!(should_look_up(&song, Trigger::Always));
    }

    /// A lookup needs both to identify the track, and the daemon drops a
    /// partial that carries neither.
    #[test]
    fn a_song_without_a_title_or_artist_is_never_looked_up() {
        let mut song = song_with(None, None);
        song.title = None;
        assert!(!should_look_up(&song, Trigger::Always));

        let mut song = song_with(None, None);
        song.artist = None;
        assert!(!should_look_up(&song, Trigger::Always));
    }

    /// The partial carries title and artist for identification -- that is how
    /// apply_song_information drops an answer that arrived after the song
    /// changed -- and names its own source, so the placeholder's marker is not
    /// left standing over the new image.
    #[test]
    fn the_partial_identifies_the_song_and_names_its_source() {
        let song = song_with(None, None);
        let partial = partial_for(&song, "llm", "https://img.example/found.jpg");

        assert_eq!(partial.title, song.title);
        assert_eq!(partial.artist, song.artist);
        assert_eq!(
            partial.cover_art_url.as_deref(),
            Some("https://img.example/found.jpg")
        );
        assert_eq!(
            partial.metadata.get(COVER_ART_SOURCE),
            Some(&serde_json::Value::String("llm".to_string()))
        );
    }
}
