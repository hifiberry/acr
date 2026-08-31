use log::{debug, info, warn};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::helpers::ArtistUpdater;
use std::sync::Arc;
use parking_lot::RwLock;
use std::collections::HashMap;

/// Looks up MusicBrainz IDs for an artist and returns them if found
/// 
/// This function searches for MusicBrainz IDs associated with the given artist name.
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to look up
/// 
/// # Returns
/// A tuple containing:
/// * `Vec<String>` - Vector of MusicBrainz IDs if found, empty vector otherwise
/// * `bool` - true if this is a partial match (only some artists in a multi-artist name found)
pub fn lookup_artist_mbids(artist_name: &str) -> (Vec<String>, bool) {
    debug!("Looking up MusicBrainz IDs for artist: {}", artist_name);
    
    // Try to retrieve MusicBrainz ID using search_mbids_for_artist function
    // This is now a fully synchronous call since we replaced musicbrainz_rs with direct HTTP
    let search_result = search_mbids_for_artist(artist_name, true, false, true);
    
    match search_result {
        MusicBrainzSearchResult::Found(mbids, _) => {
            debug!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, false) // Complete match
        },
        MusicBrainzSearchResult::FoundPartial(mbids, _) => {
            info!("Found {} partial MusicBrainz ID(s) for multi-artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, true) // Partial match
        },
        MusicBrainzSearchResult::NotFound => {
            info!("No MusicBrainz ID found for artist: {}", artist_name);
            (Vec::new(), false)
        },
        MusicBrainzSearchResult::Error(error) => {
            warn!("Error retrieving MusicBrainz ID for artist {}: {}", artist_name, error);
            (Vec::new(), false)
        }
    }
}

/// Download and cache artist images using the cover art system
/// 
/// This function retrieves artist images using the new artist store module.
/// 
/// # Arguments
/// * `artist` - The artist to update with cover art
/// 
/// # Returns
/// The updated artist with image URLs in metadata
fn update_artist_with_coverart(artist: Artist) -> Artist {
    debug!("Updating artist {} with cover art system", artist.name);
    
    // Use the new artist store to handle cover art
    crate::helpers::artist_store::update_artist_with_coverart(artist)
}

/// Updates artist data by fetching additional information like MusicBrainz IDs
/// 
/// This function takes an artist and attempts to retrieve and set any missing data
/// such as MusicBrainz IDs.
/// 
/// # Arguments
/// * `artist` - The artist to update
/// 
/// # Returns
/// The updated artist
pub fn update_data_for_artist(mut artist: Artist) -> Artist {
    debug!("Updating data for artist: {}", artist.name);
    
    // Check if the artist already has MusicBrainz IDs set
    let has_mbid = match &artist.metadata {
        Some(meta) => !meta.mbid.is_empty(),
        None => false,
    };
      if !has_mbid {
        debug!("No MusicBrainz ID set for artist {}, attempting to retrieve it", artist.name);
        
        // Use the synchronous function to look up MusicBrainz IDs directly
        // No more need for Tokio runtime since our function is now synchronous
        let (mbids, partial_match) = lookup_artist_mbids(&artist.name);
        let mbid_count = mbids.len();
        
        // Add each MusicBrainz ID to the artist if any were found
        for mbid in mbids {
            artist.add_mbid(mbid);
        }

        // if there is more than one mbid or it was a partial match, it's a multi-artist entry
        if mbid_count > 1 || partial_match {
            artist.is_multi = true; // Mark as multi-artist entry
            artist.clear_metadata(); // Clear metadata for multi-artist entries
            debug!("Cleared metadata for multi-artist entry: {}", artist.name);
        } else if mbid_count > 0 {
            info!("Updated artist '{}' with MusicBrainz data: {} ID(s)", artist.name, mbid_count);
            debug!("Added MusicBrainz ID(s) to artist {}", artist.name);
        }
        
        // Record if this is a partial match in the artist metadata
        if partial_match {
            debug!("Partial match found for multi-artist name: {}", artist.name);
            if let Some(meta) = &mut artist.metadata {
                meta.is_partial_match = true;
            }
        }
    } else {
        debug!("Artist {} already has MusicBrainz ID(s)", artist.name);
    }
    
    // If the artist has MusicBrainz IDs, update from the coverart system
    if artist.metadata.as_ref().is_some_and(|meta| !meta.mbid.is_empty()) {
        debug!("Artist {} has MusicBrainz ID(s), updating with cover art system", artist.name);
        artist = update_artist_with_coverart(artist);
    } else {
        // For artists without MusicBrainz IDs, still try coverart system with artist name only
        debug!("Artist {} has no MusicBrainz ID, trying cover art by name only", artist.name);
        artist = update_artist_with_coverart(artist);
    }
    
    // Update with individual service providers for biography and additional metadata
    // Note: The coverart system handles images, but we need individual services for biography
    
    // Check if we need biography data or genre data
    let needs_biography = artist.metadata.as_ref().is_none_or(|meta| meta.biography.is_none());
    let needs_genres = artist.metadata.as_ref().is_none_or(|meta| meta.genres.is_empty());
    
    if needs_biography || needs_genres {
        debug!("Artist {} needs biography or genre data, calling individual service updaters", artist.name);
        
        // Track what we had before updating
        let had_biography_before = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
        let genres_count_before = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
        
        // Try LastFM first for biography and genres (usually has good data)
        let lastfm_updater = crate::helpers::lastfm::LastfmUpdater;
        artist = lastfm_updater.update_artist(artist);
        
        // Check what we got from LastFM
        let has_biography_after_lastfm = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
        let genres_count_after_lastfm = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
        
        if !had_biography_before && has_biography_after_lastfm {
            info!("Downloaded biography for artist '{}' from LastFM", artist.name);
        }
        if genres_count_after_lastfm > genres_count_before {
            let new_genres = genres_count_after_lastfm - genres_count_before;
            info!("Downloaded {} genre(s) for artist '{}' from LastFM", new_genres, artist.name);
        }
        
        // Check what we still need after LastFM
        let still_needs_biography = artist.metadata.as_ref().is_none_or(|meta| meta.biography.is_none());
        let still_needs_genres = artist.metadata.as_ref().is_none_or(|meta| meta.genres.is_empty());
        let has_mbid = artist.metadata.as_ref().is_some_and(|meta| !meta.mbid.is_empty());
        
        // If we still need data and have MusicBrainz ID, try TheAudioDB
        if (still_needs_biography || still_needs_genres) && has_mbid {
            debug!("Artist {} still needs biography or genres and has MBID, trying TheAudioDB", artist.name);
            
            // Track what we have before TheAudioDB
            let had_biography_before_tadb = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
            let genres_count_before_tadb = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
            
            let theaudiodb_updater = crate::helpers::theaudiodb::TheAudioDbUpdater;
            artist = theaudiodb_updater.update_artist(artist);
            
            // Check what we got from TheAudioDB
            let has_biography_after_tadb = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
            let genres_count_after_tadb = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
            
            if !had_biography_before_tadb && has_biography_after_tadb {
                info!("Downloaded biography for artist '{}' from TheAudioDB", artist.name);
            }
            if genres_count_after_tadb > genres_count_before_tadb {
                let new_genres = genres_count_after_tadb - genres_count_before_tadb;
                info!("Downloaded {} genre(s) for artist '{}' from TheAudioDB", new_genres, artist.name);
            }
        }
        
        // FanArt.tv updater no longer provides metadata - all image handling is done by CoverartProvider
        if has_mbid {
            debug!("Artist {} has MBID - FanArt.tv images will be handled by CoverartProvider", artist.name);
        }
    } else {
        debug!("Artist {} already has biography and genre data", artist.name);
    }
    
    // Handle artists without MusicBrainz IDs but with existing thumbnails
    if artist.metadata.as_ref().is_some_and(|meta| meta.mbid.is_empty()) {
        // Check if the artist has thumbnail images
        let has_thumbnails = match &artist.metadata {
            Some(meta) => !meta.thumb_url.is_empty(),
            None => false,
        };
        
        if has_thumbnails {
            debug!("Artist {} has thumbnail image(s) but no MusicBrainz ID, skipping updates", artist.name);
        }
    }

    // Store the updated metadata in cache
    if let Some(metadata) = &artist.metadata {
        // Create a cache key using the artist's name
        let cache_key = format!("artist::metadata::{}", artist.name);
        
        // Store the metadata in the attribute cache
        match crate::helpers::attributecache::set(&cache_key, metadata) {
            Ok(_) => debug!("Stored metadata for artist {} in attribute cache", artist.name),
            Err(e) => warn!("Failed to store metadata for artist {} in attribute cache: {}", artist.name, e),
        }
        
        // If the artist has MusicBrainz IDs, store them separately for faster lookup
        if !metadata.mbid.is_empty() {
            let mbid_key = format!("artist::mbid::{}", artist.name);
            if let Err(e) = crate::helpers::attributecache::set(&mbid_key, &metadata.mbid) {
                warn!("Failed to store MusicBrainz IDs for artist {} in attribute cache: {}", artist.name, e);
            }
        }
    }
    
    // Return the potentially updated artist
    artist
}

/// Store an updated artist and record a real change.
///
/// `Artist`'s own `PartialEq` compares only `id` (it is used for identity —
/// dedup and lookup — not content), and `Artist` cannot derive a
/// content-based `PartialEq` because it holds lock-bearing fields. So the
/// change check compares `ArtistMeta`, which holds none and derives
/// `PartialEq` cleanly, plus the `is_multi` flag: both are part of what a
/// client observes (`Artist`'s `Serialize` impl includes both), and
/// `is_multi` can flip independently of metadata content (e.g. clearing an
/// already-empty `metadata` when a name turns out to be multi-artist).
///
/// Comparing against the collection's current entry — not the snapshot
/// taken at the start of the sweep — is what actually tells us whether this
/// write changes what a client can observe. The write and the bump live in
/// one function so the two cannot drift apart, matching `set_genres` on the
/// album path.
fn store_artist(
    map: &mut HashMap<String, Artist>,
    name: String,
    artist: Artist,
    version: Option<&crate::data::library::LibraryVersion>,
) {
    let changed = match map.get(&name) {
        Some(existing) => existing.metadata != artist.metadata || existing.is_multi != artist.is_multi,
        None => true,
    };
    map.insert(name, artist);
    if changed {
        if let Some(version) = version {
            version.bump();
        }
    }
}

/// Start a background thread to update metadata for all artists in the library sequentially
///
/// This function updates artist metadata using the update_data_for_artist method in a background process.
/// It takes an Arc to the artists collection for direct updating and reading.
///
/// # Arguments
/// * `artists_collection` - Arc to the artists collection for updating
pub fn update_library_artists_metadata_in_background(
    artists_collection: Arc<RwLock<HashMap<String, Artist>>>,
    version: Option<crate::data::library::LibraryVersion>,
) {
    debug!("Starting background thread to update artist metadata");
    
    // Spawn a new thread to handle the metadata updates
    use std::thread;
    thread::spawn(move || {
        let job_id = "artist_metadata_update".to_string();
        let job_name = "Artist Metadata Update".to_string();
        
        // Register the background job
        if let Err(e) = crate::helpers::backgroundjobs::register_job(job_id.clone(), job_name) {
            warn!("Failed to register background job: {}", e);
            return;
        }
        
        info!("Artist metadata update thread started");

        // Get all artists from the collection
        let artists = {
            let artists_map = artists_collection.read();
            // Clone all artists for processing
            artists_map.values().cloned().collect::<Vec<_>>()
        };

        let total = artists.len();
        info!("Processing metadata for {} artists", total);
        
        // Update the job with total count
        if let Err(e) = crate::helpers::backgroundjobs::update_job(
            &job_id,
            Some(format!("Starting metadata update for {} artists", total)),
            Some(0),
            Some(total)
        ) {
            warn!("Failed to update background job: {}", e);
        }

        for (index, artist) in artists.into_iter().enumerate() {
            let artist_name = artist.name.clone();
            debug!("Updating metadata for artist: {}", artist_name);
            
            // Update progress in background job
            let completed = index;
            let progress_message = format!("Processing artist: {}", artist_name);
            if let Err(e) = crate::helpers::backgroundjobs::update_job(
                &job_id,
                Some(progress_message),
                Some(completed),
                Some(total)
            ) {
                warn!("Failed to update background job progress: {}", e);
            }

            // Use the synchronous version of update_data_for_artist
            let updated_artist = update_data_for_artist(artist);

            // Check if we found new metadata to log appropriately
            let has_new_metadata = {
                let original_metadata = {
                    let artists_map = artists_collection.read();
                    artists_map.get(&artist_name).and_then(|a| a.metadata.clone())
                };

                if let Some(new_metadata) = &updated_artist.metadata {
                    if !new_metadata.mbid.is_empty() {
                        match original_metadata {
                            Some(old_meta) if !old_meta.mbid.is_empty() => false,
                            _ => {
                                info!("Adding MusicBrainz ID(s) to artist {}", artist_name);
                                true
                            }
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            // Update the artist in the collection
            {
                let mut artists_map = artists_collection.write();
                store_artist(&mut artists_map, artist_name.clone(), updated_artist, version.as_ref());

                if has_new_metadata {
                    debug!("Successfully updated artist {} in library collection", artist_name);
                }
            }

            // Log progress periodically
            let count = index + 1;
            if count % 10 == 0 || count == total {
                info!("Processed {}/{} artists for metadata", count, total);
                
                // Update background job with milestone progress
                if let Err(e) = crate::helpers::backgroundjobs::update_job(
                    &job_id,
                    Some(format!("Processed {}/{} artists", count, total)),
                    Some(count),
                    Some(total)
                ) {
                    warn!("Failed to update background job milestone: {}", e);
                }
            }
            
            // Sleep between updates to avoid overwhelming external services
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        info!("Artist metadata update process completed");
        
        // Complete and remove the background job
        if let Err(e) = crate::helpers::backgroundjobs::complete_job(&job_id) {
            warn!("Failed to complete background job: {}", e);
        }
    });

    info!("Background artist metadata update initiated");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::library::LibraryVersion;
    use crate::data::metadata::ArtistMeta;
    use crate::data::Identifier;

    fn artist(name: &str, metadata: Option<ArtistMeta>, is_multi: bool) -> Artist {
        Artist {
            id: Identifier::Numeric(1),
            name: name.to_string(),
            is_multi,
            metadata,
        }
    }

    #[test]
    fn storing_a_changed_artist_stores_it_and_bumps_the_version() {
        let version = LibraryVersion::new();
        let mut map = HashMap::new();
        map.insert("Radiohead".to_string(), artist("Radiohead", None, false));

        let mut meta = ArtistMeta::new();
        meta.add_genre("rock".to_string());
        let updated = artist("Radiohead", Some(meta.clone()), false);

        store_artist(&mut map, "Radiohead".to_string(), updated, Some(&version));

        assert_eq!(map.get("Radiohead").unwrap().metadata, Some(meta));
        assert_eq!(version.get(), 1, "a real change must move the version");
    }

    #[test]
    fn storing_an_unchanged_artist_still_stores_it_but_does_not_bump() {
        // Storing unconditionally (rather than skipping) keeps the map's contents
        // exactly what was just computed even if a comparison bug ever treats two
        // genuinely-equal values as equal for the wrong reason; only the *version*
        // bump is conditional on there being a real change.
        let version = LibraryVersion::new();
        let mut meta = ArtistMeta::new();
        meta.add_genre("rock".to_string());

        let mut map = HashMap::new();
        map.insert("Radiohead".to_string(), artist("Radiohead", Some(meta.clone()), false));

        let unchanged = artist("Radiohead", Some(meta.clone()), false);
        store_artist(&mut map, "Radiohead".to_string(), unchanged, Some(&version));

        assert_eq!(map.get("Radiohead").unwrap().metadata, Some(meta));
        assert_eq!(version.get(), 0, "no content changed, so the version must not move");
    }

    #[test]
    fn storing_with_no_version_stores_and_never_bumps() {
        // The LMS path: no LibraryVersion is tracked, so `version` is `None`.
        let mut map = HashMap::new();
        map.insert("Radiohead".to_string(), artist("Radiohead", None, false));

        let mut meta = ArtistMeta::new();
        meta.add_genre("rock".to_string());
        let updated = artist("Radiohead", Some(meta.clone()), false);

        store_artist(&mut map, "Radiohead".to_string(), updated, None);

        assert_eq!(map.get("Radiohead").unwrap().metadata, Some(meta));
    }
}
