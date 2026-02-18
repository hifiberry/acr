// Mapping utilities for converting between LMS-specific data structures and
// the common data structures used throughout the application.
use std::sync::Arc;
use parking_lot::Mutex;
use chrono::NaiveDate;
use log::warn;

use crate::data::Identifier;
use crate::data::album::Album as AcrAlbum;
use crate::data::artist::Artist as AcrArtist;
use crate::data::track::Track as AcrTrack;

use super::jsonrps::{Album as LmsAlbum, Artist as LmsArtist, Track as LmsTrack};

/// Maps an LMS Album to the application's Album structure
pub fn map_album(lms_album: &LmsAlbum) -> Option<AcrAlbum> {
    // Extract the album ID and title, which are required fields
    let id = match &lms_album.id {
        Some(id_str) => id_str.clone(),
        None => return None, // Can't create an album without an ID
    };
    
    let name = match &lms_album.title {
        Some(title) => title.clone(),
        None => return None, // Can't create an album without a title
    };
    
    // Create the album with the required fields
    let mut album = AcrAlbum {
        id: Identifier::String(id),
        name,
        artists: Arc::new(Mutex::new(Vec::new())),
        artists_flat: None,
        release_date: None,
        tracks: Arc::new(Mutex::new(Vec::new())),
        cover_art: None,
        uri: None,
    };
    
    // Add any artist information if available
    if let Some(artist_name) = &lms_album.artist {
        let mut artists = album.artists.lock();
        artists.push(artist_name.clone());
        album.artists_flat = Some(artist_name.clone());
    }
    
    // Add release year if available
    if let Some(year_str) = &lms_album.year {
        // Try to parse the year in various formats
        // First try as a simple year (YYYY)
        if let Ok(year) = year_str.parse::<i32>() {
            album.release_date = NaiveDate::from_ymd_opt(year, 1, 1);
        } 
        // Otherwise try ISO format (YYYY-MM-DD)
        else if let Ok(date) = NaiveDate::parse_from_str(year_str, "%Y-%m-%d") {
            album.release_date = Some(date);
        }
    }
    
    // Add cover art URL if available
    if let Some(artwork_url) = &lms_album.artwork_url {
        album.cover_art = Some(artwork_url.clone());
    }
    
    Some(album)
}

/// Maps an LMS Track to the application's Track structure
pub fn map_track(lms_track: &LmsTrack, album_artist: Option<&str>) -> AcrTrack {
    // Create a basic track with the title
    let mut track = AcrTrack::with_name(lms_track.title.clone());
    
    // Try to extract a track number
    // LMS doesn't directly provide disc/track numbers in the standard track response
    // but we can try to infer it if available in other fields or from the ID
    if let Ok(track_num) = lms_track.id.parse::<u16>() {
        track.track_number = Some(track_num);
    }
    
    // Add artist if different from album artist
    if !lms_track.artist.is_empty() {
        if let Some(album_artist_name) = album_artist {
            if lms_track.artist != album_artist_name {
                track.artist = Some(lms_track.artist.clone());
            }
        } else {
            track.artist = Some(lms_track.artist.clone());
        }
    }
    
    // Set duration if available
    if let Some(_duration_secs) = lms_track.duration {
        // We don't need to modify the duration, as both use seconds
    }
    
    // Set URI if we can construct one
    // LMS often doesn't provide direct file URIs in basic track responses
    // but we might be able to construct one if needed in the future

    track
}

/// Maps an LMS Artist to the application's Artist structure
pub fn map_artist(lms_artist: &LmsArtist) -> AcrArtist {
    // Create a new artist with the ID and name from LMS
    let artist = AcrArtist {
        id: Identifier::String(lms_artist.id.clone()),
        name: lms_artist.artist.clone(),
        // Check if this is a multi-artist entry (contains comma in the name)
        is_multi: lms_artist.artist.contains(','),
        metadata: None,
    };
    
    artist
}

/// Maps a collection of LMS albums to the application's Album structures
pub fn map_albums(lms_albums: &[LmsAlbum]) -> Vec<AcrAlbum> {
    let mut result = Vec::with_capacity(lms_albums.len());
    
    for lms_album in lms_albums {
        if let Some(album) = map_album(lms_album) {
            result.push(album);
        } else {
            warn!("Failed to map LMS album: {:?}", lms_album);
        }
    }
    
    result
}

/// Maps a collection of LMS tracks to a single album with tracks
pub fn map_tracks_to_album(
    album_id: String, 
    album_name: String, 
    album_artist: Option<String>, 
    lms_tracks: &[LmsTrack]
) -> AcrAlbum {
    // Create a new album
    let mut album = AcrAlbum {
        id: Identifier::String(album_id),
        name: album_name,
        artists: Arc::new(Mutex::new(Vec::new())),
        artists_flat: None,
        release_date: None,
        tracks: Arc::new(Mutex::new(Vec::new())),
        cover_art: None,
        uri: None,
    };
    
    // Add album artist if available
    if let Some(artist) = album_artist {
        let mut artists = album.artists.lock();
        artists.push(artist.clone());
        album.artists_flat = Some(artist);
    }

    // Add all tracks to the album
    {
        let mut album_tracks = album.tracks.lock();
        for lms_track in lms_tracks {
            let acr_track = map_track(lms_track, album.artists_flat.as_deref());
            album_tracks.push(acr_track);
        }
    }
    
    // Sort the tracks by disc and track number
    album.sort_tracks();
    
    // Try to set cover art from the first track if available
    if let Some(first_track) = lms_tracks.first() {
        if !first_track.coverid.is_empty() {
            // LMS uses coverid which needs to be converted to a URL
            // This would typically be handled by a separate method or system
            // but we'll use a placeholder format here
            album.cover_art = Some(format!("/music/{}/cover", first_track.coverid));
        }
    }
    
    album
}

/// Maps all albums by a given artist
pub fn map_artist_with_albums(
    lms_artist: &LmsArtist,
    lms_albums: &[LmsAlbum]
) -> (AcrArtist, Vec<AcrAlbum>) {
    // Map the artist
    let artist = map_artist(lms_artist);
    
    // Map all albums
    let albums = lms_albums.iter()
        .filter_map(|album| {
            // Only include albums where this artist is the main artist
            if let Some(album_artist) = &album.artist {
                if album_artist == &lms_artist.artist {
                    return map_album(album);
                }
            }
            // Still include the album if the artist ID matches
            // (This might be redundant but included for completeness)
            map_album(album)
        })
        .collect();
    
    (artist, albums)
}