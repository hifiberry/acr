// Copyright (c) 2020 Modul 9/HiFiBerry
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! FanArt.tv API client for retrieving album and artist artwork
//! Based on the original Python implementation from audiocontrol2

use serde_json::Value;
use log::{debug, warn};
use reqwest;

// API key for fanart.tv
const APIKEY: &str = "749a8fca4f2d3b0462b287820ad6ab06";

/// Get the cover URL from FanArt.tv for an artist or album
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// * `album_mbid` - MusicBrainz ID of the album
/// * `allow_artist_picture` - If true, return artist picture if no album cover is found
/// 
/// # Returns
/// * `Option<String>` - URL of the cover if found, otherwise None
pub fn get_fanart_cover(artist_mbid: &str, album_mbid: Option<&str>, allow_artist_picture: bool) -> Option<String> {
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        APIKEY
    );

    let client = reqwest::blocking::Client::new();
    match client.get(&url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                debug!("Artist does not exist on fanart.tv (HTTP status: {})", response.status());
                return None;
            }

            match response.json::<Value>() {
                Ok(data) => {
                    // Try to find the album cover first if we have an album MBID
                    if let Some(album_id) = album_mbid {
                        if let Some(albums) = data.get("albums") {
                            if let Some(album) = albums.get(album_id) {
                                if let Some(album_cover) = album.get("albumcover") {
                                    if let Some(url) = album_cover.get("url")
                                        .and_then(|u| u.as_str()) {
                                        debug!("Found album cover on fanart.tv");
                                        return Some(url.to_string());
                                    }
                                }
                            }
                        }
                        debug!("Found no album cover on fanart.tv");
                    }
                    
                    // If no album cover exists and artist pictures are allowed, use artist thumb
                    if allow_artist_picture {
                        if let Some(artist_thumbs) = data.get("artistthumb") {
                            if let Some(thumb) = artist_thumbs.get(1) {
                                if let Some(url) = thumb.get("url")
                                    .and_then(|u| u.as_str()) {
                                    debug!("Found artist picture on fanart.tv");
                                    return Some(url.to_string());
                                }
                            }
                        }
                        debug!("Found no artist picture on fanart.tv");
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv: {}", e);
                }
            }
        }
        Err(e) => {
            warn!("Couldn't retrieve data from fanart.tv: {}", e);
        }
    }

    None
}
