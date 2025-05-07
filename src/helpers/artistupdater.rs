use log::{debug, info, warn, error};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::helpers::theartistdb;
use std::sync::Arc;
use std::thread;

/// Trait for services that can update artist metadata
pub trait ArtistUpdater {
    /// Update an artist with additional metadata from a service
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// 
    /// # Returns
    /// The updated artist with additional metadata
    fn update_artist(&self, artist: Artist) -> Artist;
}

/// Create a "clean" artist name without unicode characters (converted to ascii), 
/// special characters or double spaces
/// convert to lowercase and trim whitespace
pub fn artist_basename(artist_name: &str) -> String {
    // Convert to ASCII (remove diacritics and other non-ascii characters)
    let ascii_name = deunicode::deunicode(artist_name);
    
    // Keep only alphanumeric characters and spaces, replace others with spaces
    let mut clean_name = String::with_capacity(ascii_name.len());
    for c in ascii_name.chars() {
        if c.is_alphanumeric() || c == ' ' {
            clean_name.push(c);
        } else {
            clean_name.push(' ');
        }
    }
    
    // Convert to lowercase
    let lowercase_name = clean_name.to_lowercase();
    
    // Remove double spaces
    let mut result = String::with_capacity(lowercase_name.len());
    let mut last_was_space = false;
    
    for c in lowercase_name.chars() {
        if c == ' ' {
            if !last_was_space {
                result.push(c);
            }
            last_was_space = true;
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    
    // Trim whitespace
    result.trim().to_string()
}

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
    let search_result = search_mbids_for_artist(artist_name, true, false, true);
    
    match search_result {
        MusicBrainzSearchResult::Found(mbids, _) => {
            debug!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, false) // Complete match
        },
        MusicBrainzSearchResult::FoundPartial(mbids, _) => {
            warn!("Found {} partial MusicBrainz ID(s) for multi-artist {}: {:?}", 
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

/// Updates an artist's thumbnails using FanArt.tv service
/// 
/// This function fetches thumbnail URLs for an artist and downloads them for caching.
/// 
/// # Arguments
/// * `artist` - The artist to update
/// * `mbid` - The MusicBrainz ID to use for looking up thumbnails
/// 
/// # Returns
/// The updated artist with thumbnail URLs
fn update_artist_thumbnails_from_fanarttv(mut artist: Artist, mbid: &str) -> Artist {
    debug!("Fetching thumbnail URLs for artist {} with MBID {}", artist.name, mbid);
    
    // Get thumbnail URLs from FanArt.tv
    let thumbnail_urls = crate::helpers::fanarttv::get_artist_thumbnails(mbid, Some(5));
    
    // Check if we have any thumbnails before trying to add them
    let has_thumbnails = !thumbnail_urls.is_empty();
    
    // Add each thumbnail URL to the artist
    if let Some(meta) = &mut artist.metadata {
        for url in &thumbnail_urls {
            meta.thumb_url.push(url.clone());
            debug!("Added thumbnail URL for artist {}", artist.name);
        }
    }
    
    // If thumbnails were found, also try to download them for caching
    if has_thumbnails {
        debug!("Downloading artist images for {}", artist.name);
        let download_result = crate::helpers::fanarttv::download_artist_images(mbid, &artist.name);
        if download_result {
            debug!("Successfully downloaded images for artist {}", artist.name);
        } else {
            debug!("Failed to download some images for artist {}", artist.name);
        }
    }
    
    artist
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
        
        // Use the extracted function to look up MusicBrainz IDs
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
        } else {
            debug!("Added MusicBrainz ID(s) to artist {}", artist.name);
        }
        
        // Record if this is a partial match in the artist metadata
        if partial_match {
            warn!("Partial match found for multi-artist name: {}", artist.name);
            if let Some(meta) = &mut artist.metadata {
                meta.is_partial_match = true;
            }
        }
    } else {
        debug!("Artist {} already has MusicBrainz ID(s)", artist.name);
    }
    
    // Check if the artist has thumbnail images
    let has_thumbnails = match &artist.metadata {
        Some(meta) => !meta.thumb_url.is_empty(),
        None => false,
    };
    
    // If the artist has MusicBrainz IDs but no thumbnails, try to get them
    if artist.metadata.as_ref().map_or(false, |meta| !meta.mbid.is_empty()) {
        // Get the first MusicBrainz ID for the artist
        let mbid_opt = artist.metadata.as_ref().and_then(|meta| meta.mbid.first().cloned());
        
        if mbid_opt.is_some() {
            // Create a TheArtistDbUpdater and use it to update the artist
            let artist_db_updater = theartistdb::TheArtistDbUpdater::new();
            artist = artist_db_updater.update_artist(artist);
            
            // If we still don't have thumbnails, try FanArt.tv
            if !has_thumbnails && artist.metadata.as_ref().map_or(true, |meta| meta.thumb_url.is_empty()) {
                debug!("No thumbnails set from TheArtistDB for artist {}, trying FanArt.tv", artist.name);
                
                // Check if there's only a single MusicBrainz ID
                let mbid_count = artist.metadata.as_ref().map_or(0, |meta| meta.mbid.len());
                
                if mbid_count > 1 {
                    debug!("Artist {} has multiple MusicBrainz IDs ({}), skipping FanArt.tv image download", artist.name, mbid_count);
                } else if let Some(mbid) = mbid_opt {
                    // Update thumbnails using the FanArt.tv function
                    artist = update_artist_thumbnails_from_fanarttv(artist, &mbid);
                }
            }
        }
    } else if has_thumbnails {
        debug!("Artist {} already has thumbnail image(s)", artist.name);
    }
    
    // Return the potentially updated artist
    artist
}

/// Start a background thread to update metadata for all artists in the library
///
/// This function takes a library that implements the LibraryInterface trait, gets all artists,
/// and updates their metadata using the update_data_for_artist method in a background thread.
///
/// # Arguments
/// * `library` - An Arc-wrapped library instance implementing the LibraryInterface trait
pub fn update_library_artists_metadata_in_background<T>(library: Arc<T>) 
where
    T: crate::data::LibraryInterface + Send + Sync + 'static,
{
    debug!("Starting background thread to update artist metadata");
    
    // Spawn a new thread to handle the metadata updates
    thread::spawn(move || {
        info!("Artist metadata update thread started");
        
        // Get all artists from the library
        let artists = library.get_artists();
        let total = artists.len();
        info!("Processing metadata for {} artists", total);
        
        let mut count = 0;
        
        // Process each artist one by one
        for artist in artists {
            debug!("Updating metadata for artist: {}", artist.name);
            
            // Use the existing update_data_for_artist function
            let updated_artist = update_data_for_artist(artist);
            
            // Save the updated artist back to the library
            if let Some(metadata) = &updated_artist.metadata {
                if !metadata.mbid.is_empty() {
                    // Only update if we actually got new MusicBrainz IDs
                    if let Some(stored_artist) = library.get_artist(&updated_artist.name) {
                        if stored_artist.metadata.is_none() || 
                           stored_artist.metadata.as_ref().map_or(true, |m| m.mbid.is_empty()) {
                            // Only print when we're actually adding new metadata
                            info!("Adding MusicBrainz ID(s) to artist {}", updated_artist.name);
                        }
                    }
                }
            }
            
            count += 1;
            if count % 10 == 0 || count == total {
                info!("Processed {}/{} artists for metadata", count, total);
            }
        }
        
        info!("Artist metadata update thread completed");
    });
    
    info!("Background artist metadata update initiated");
}