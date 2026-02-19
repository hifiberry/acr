use std::collections::HashMap;
use base64::{Engine as _, engine::general_purpose};

#[derive(Debug)]
pub enum ShairportMessage {
    Control(String),
    SessionStart(String),
    SessionEnd(String),
    ChunkData {
        chunk_id: u32,
        total_chunks: u32,
        data_type: String,
        data: Vec<u8>,
    },
    CompletePicture {
        data: Vec<u8>,
        format: String,
    },
    Unknown(Vec<u8>),
}

#[derive(Debug)]
pub struct ChunkCollector {
    chunks: HashMap<u32, Vec<u8>>, // chunk_id -> data
    pub total_chunks: u32,
}

impl ChunkCollector {
    pub fn new(total_chunks: u32, _data_type: String) -> Self {
        Self {
            chunks: HashMap::new(),
            total_chunks,
        }
    }
    
    pub fn add_chunk(&mut self, chunk_id: u32, data: Vec<u8>) -> Option<Vec<u8>> {
        self.chunks.insert(chunk_id, data);
        
        // Check if we have all chunks
        if self.chunks.len() as u32 == self.total_chunks {
            // Combine chunks in order
            let mut combined = Vec::new();
            for i in 0..self.total_chunks {
                if let Some(chunk_data) = self.chunks.get(&i) {
                    combined.extend_from_slice(chunk_data);
                } else {
                    return None; // Missing chunk
                }
            }
            Some(combined)
        } else {
            None
        }
    }
}

pub fn parse_shairport_message(data: &[u8]) -> ShairportMessage {
    // Try to parse binary chunk data first (this takes priority)
    if data.len() >= 24 && &data[0..8] == b"ssncchnk" {
        // Parse chunk header: "ssncchnk" + chunk_id (4 bytes) + total_chunks (4 bytes) + data_type (8 bytes)
        let chunk_id = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let total_chunks = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        
        // Extract data type (next 8 bytes after header)
        let data_type = String::from_utf8_lossy(&data[16..24]).to_string();
        
        // Skip null bytes in the payload to find actual data
        let mut payload_start = 24;
        while payload_start < data.len() && data[payload_start] == 0 {
            payload_start += 1;
        }
        
        let payload = if payload_start < data.len() {
            data[payload_start..].to_vec()
        } else {
            // No actual data, just padding
            Vec::new()
        };
        
        return ShairportMessage::ChunkData {
            chunk_id,
            total_chunks,
            data_type,
            data: payload,
        };
    }
    
    // Extract command (first 8 bytes) and payload (rest)
    if data.len() >= 8 {
        let command = &data[0..8];
        let payload = &data[8..];
        
        // Handle commands that are exactly 8 bytes (no payload)
        match command {
            b"ssncpaus" => return ShairportMessage::Control("PAUSE".to_string()),
            b"ssncpres" => return ShairportMessage::Control("RESUME".to_string()),
            b"ssncaend" => return ShairportMessage::Control("SESSION_END".to_string()),
            b"ssncabeg" => return ShairportMessage::Control("AUDIO_BEGIN".to_string()),
            b"ssncpbeg" => return ShairportMessage::Control("PLAYBACK_BEGIN".to_string()),
            b"ssncPICT" => return ShairportMessage::Control("PICTURE_REQUEST".to_string()),
            _ => {}
        }
        
        // Handle commands with payloads
        match command {
            // Session start/end with IDs  
            b"ssncmdst" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("METADATA_START: {}", content));
                }
            },
            b"ssncmden" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("METADATA_END: {}", content));
                }
            },
            
            // Connection info (UTF-8 payload)
            b"ssncdisc" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("DISCOVERED: {}", content));
                }
            },
            b"ssncconn" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CONNECTED: {}", content));
                }
            },
            b"ssncclip" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_IP: {}", content));
                }
            },
            b"ssncsvip" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SERVER_IP: {}", content));
                }
            },
            b"ssncsnam" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SERVER_NAME: {}", content));
                }
            },
            
            // Playback control (UTF-8 payload)
            b"ssncpvol" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("VOLUME: {}", content));
                }
            },
            b"ssncprgr" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("PROGRESS: {}", content));
                }
            },
            
            // Core metadata (UTF-8 payload) - from iTunes, etc.
            b"coreasal" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("ALBUM: {}", content));
                }
            },
            b"coreasar" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("ARTIST: {}", content));
                }
            },
            b"coreminm" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("TRACK: {}", content));
                }
            },
            b"coreascp" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("COMPOSER: {}", content));
                } else {
                    return ShairportMessage::Control("COMPOSER: (empty)".to_string());
                }
            },
            b"coreasgn" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("GENRE: {}", content));
                } else {
                    return ShairportMessage::Control("GENRE: (empty)".to_string());
                }
            },
            b"coreassl" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("ALBUM_ARTIST: {}", content));
                }
            },
            b"coreascm" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("COMMENT: {}", content));
                }
            },
            b"coreasdt" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SONG_DESCRIPTION: {}", content));
                }
            },
            b"coreasaa" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SONG_ALBUM_ARTIST: {}", content));
                }
            },
            b"coreassn" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SORT_NAME: {}", content));
                }
            },
            b"coreassa" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SORT_ARTIST: {}", content));
                }
            },
            b"coreassu" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SORT_ALBUM: {}", content));
                }
            },
            b"coreassc" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("SORT_COMPOSER: {}", content));
                }
            },
            
            // Client/session information (UTF-8 payload)
            b"ssncflsr" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("FRAME_SEQUENCE_REFERENCE: {}", content));
                }
            },
            b"ssncpfls" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("PREVIOUS_FRAME_SEQUENCE: {}", content));
                }
            },
            
            // Binary core metadata messages (binary payload)
            b"coreasdk" => {
                // Song Data Kind - single byte value
                if !payload.is_empty() {
                    let song_data_kind = payload[0];
                    return ShairportMessage::Control(format!("SONG_DATA_KIND: {}", song_data_kind));
                }
            },
            b"coremper" => {
                // Item ID - 64-bit value (8 bytes)
                if payload.len() >= 8 {
                    let high = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    let low = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                    let item_id = ((high as u64) << 32) | (low as u64);
                    return ShairportMessage::Control(format!("ITEM_ID: {:016x}", item_id));
                }
            },
            b"coreastm" => {
                // Song Time in milliseconds - 32-bit value
                if payload.len() >= 4 {
                    let time_ms = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    return ShairportMessage::Control(format!("SONG_TIME_MS: {}", time_ms));
                }
            },
            b"coreastn" => {
                // Track number - 16-bit value
                if payload.len() >= 2 {
                    let track_num = u16::from_be_bytes([payload[0], payload[1]]);
                    return ShairportMessage::Control(format!("TRACK_NUMBER: {}", track_num));
                }
            },
            b"coreastc" => {
                // Track count - 16-bit value (same format as track number)
                if payload.len() >= 2 {
                    let track_count = u16::from_be_bytes([payload[0], payload[1]]);
                    return ShairportMessage::Control(format!("TRACK_COUNT: {}", track_count));
                }
            },
            b"corecaps" => {
                // Capabilities - single byte value
                if !payload.is_empty() {
                    let capability = payload[0];
                    return ShairportMessage::Control(format!("CAPABILITIES: {}", capability));
                }
            },
            
            // Additional DACP and ShairportSync message types
            b"ssncdapo" => {
                // DACP Port
                if payload.len() >= 2 {
                    let port = u16::from_be_bytes([payload[0], payload[1]]);
                    return ShairportMessage::Control(format!("DACP_PORT: {}", port));
                } else {
                    return ShairportMessage::Control("DACP_PORT: (empty)".to_string());
                }
            },
            b"ssncdaid" => {
                // DACP ID 
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("DACP_ID: {}", content));
                }
            },
            b"ssncacre" => {
                // Active Remote
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("ACTIVE_REMOTE: {}", content));
                }
            },
            b"ssncsnua" => {
                // User Agent
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("USER_AGENT: {}", content));
                }
            },
            b"ssnccdid" => {
                // Client Device ID
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_DEVICE_ID: {}", content));
                }
            },
            b"ssnccmod" => {
                // Client Model
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_MODEL: {}", content));
                }
            },
            b"ssnccmac" => {
                // Client MAC
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_MAC: {}", content));
                }
            },
            b"ssncphbt" => {
                // Frame position
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("FRAME_POSITION: {}", content));
                }
            },
            b"ssncphb0" => {
                // First frame position
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("FIRST_FRAME_POSITION: {}", content));
                }
            },
            b"ssncstyp" => {
                // Stream type
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("STREAM_TYPE: {}", content));
                }
            },
            b"ssncpcst" => {
                // Picture start
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("PICTURE_START: {}", content));
                } else {
                    return ShairportMessage::Control("PICTURE_START".to_string());
                }
            },
            b"ssncpcen" => {
                // Picture end
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("PICTURE_END: {}", content));
                } else {
                    return ShairportMessage::Control("PICTURE_END".to_string());
                }
            },
            _ => {}
        }
    }
    
    // Try to parse as UTF-8 text for shorter messages or unknown formats
    if let Ok(text) = std::str::from_utf8(data) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            // For unknown text, we'll still return it as Unknown with the raw data
            // so the display layer can show both text and hex dump
            return ShairportMessage::Unknown(data.to_vec());
        }
    }

    // If nothing else matches, it's unknown binary data
    ShairportMessage::Unknown(data.to_vec())
}

pub fn detect_image_format(data: &[u8]) -> String {
    if data.len() >= 4 {
        match &data[0..4] {
            [0xFF, 0xD8, 0xFF, _] => "JPEG".to_string(),
            [0x89, 0x50, 0x4E, 0x47] => "PNG".to_string(), // PNG signature
            [0x47, 0x49, 0x46, 0x38] => "GIF".to_string(), // GIF87a or GIF89a
            [0x42, 0x4D, _, _] => "BMP".to_string(), // BMP
            _ => {
                if data.len() >= 12 && &data[4..12] == b"ftypheic" {
                    "HEIC".to_string()
                } else if data.len() >= 8 && &data[0..8] == b"RIFF" {
                    "WEBP".to_string()
                } else {
                    "Unknown".to_string()
                }
            }
        }
    } else {
        "Unknown".to_string()
    }
}

pub fn get_image_dimensions(data: &[u8], format: &str) -> String {
    match format {
        "JPEG" => get_jpeg_dimensions(data),
        "PNG" => get_png_dimensions(data),
        _ => "Unknown".to_string(),
    }
}

pub fn get_jpeg_dimensions(data: &[u8]) -> String {
    let mut i = 2; // Skip initial 0xFF 0xD8
    
    while i + 4 < data.len() {
        if data[i] == 0xFF {
            let marker = data[i + 1];
            
            // SOF0, SOF1, SOF2 markers contain dimension info
            if (0xC0..=0xC3).contains(&marker)
                && i + 9 < data.len() {
                    let height = u16::from_be_bytes([data[i + 5], data[i + 6]]);
                    let width = u16::from_be_bytes([data[i + 7], data[i + 8]]);
                    return format!("{}x{}", width, height);
                }
            
            // Skip this segment
            if i + 3 < data.len() {
                let length = u16::from_be_bytes([data[i + 2], data[i + 3]]);
                i += length as usize + 2;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }
    
    "Unknown".to_string()
}

pub fn get_png_dimensions(data: &[u8]) -> String {
    // PNG IHDR chunk starts at byte 8 and contains width/height at bytes 16-23
    if data.len() >= 24 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        format!("{}x{}", width, height)
    } else {
        "Unknown".to_string()
    }
}

use crate::data::song::Song;

/// Update a Song object with metadata from a ShairportSync message
/// Returns true if any field was updated
pub fn update_song_from_message(song: &mut Song, message: &ShairportMessage) -> bool {
    match message {
        ShairportMessage::Control(action) => {
            // Parse control messages for metadata
            if let Some((key, value)) = action.split_once(": ") {
                match key {
                    "TRACK" => {
                        song.title = Some(value.to_string());
                        true
                    }
                    "ARTIST" => {
                        song.artist = Some(value.to_string());
                        true
                    }
                    "ALBUM" => {
                        song.album = Some(value.to_string());
                        true
                    }
                    "GENRE" => {
                        song.genre = Some(value.to_string());
                        // Also add to genres vec if not already present
                        if !song.genres.contains(&value.to_string()) {
                            song.genres.push(value.to_string());
                        }
                        true
                    }
                    "COMPOSER" => {
                        if value != "(empty)" {
                            song.composer = Some(value.to_string());
                        }
                        true
                    }
                    "ALBUM_ARTIST" | "SONG_ALBUM_ARTIST" => {
                        song.album_artist = Some(value.to_string());
                        true
                    }
                    "TRACK_NUMBER" => {
                        if let Ok(track_num) = value.parse::<i32>() {
                            song.track_number = Some(track_num);
                            true
                        } else {
                            false
                        }
                    }
                    "TRACK_COUNT" => {
                        if let Ok(track_count) = value.parse::<i32>() {
                            song.total_tracks = Some(track_count);
                            true
                        } else {
                            false
                        }
                    }
                    "SORT_NAME" | "SORT_ARTIST" | "SORT_ALBUM" | "SORT_COMPOSER" => {
                        // Ignore sort fields
                        false
                    }
                    "PICTURE_START" | "PICTURE_END" | "METADATA_START" | "METADATA_END" | 
                    "ITEM_ID" | "SONG_DATA_KIND" | "FRAME_SEQUENCE_REFERENCE" | 
                    "PREVIOUS_FRAME_SEQUENCE" | "CLIENT_IP" | "SERVER_IP" | "SERVER_NAME" |
                    "DACP_PORT" | "DACP_ID" | "ACTIVE_REMOTE" | "PROGRESS" | "USER_AGENT" |
                    "CLIENT_DEVICE_ID" | "CLIENT_MODEL" | "CLIENT_MAC" | "FRAME_POSITION" |
                    "FIRST_FRAME_POSITION" | "STREAM_TYPE" | "SONG_TIME_MS" | "CAPABILITIES" |
                    "DISCOVERED" | "CONNECTED" => {
                        // Ignore internal protocol messages
                        false
                    }
                    _ => {
                        // Store other metadata in the metadata HashMap
                        if !value.trim().is_empty() && value != "(empty)" {
                            song.metadata.insert(key.to_string(), serde_json::Value::String(value.to_string()));
                            true
                        } else {
                            false
                        }
                    }
                }
            } else {
                false
            }
        }
        ShairportMessage::ChunkData { data_type, data, .. } => {
            let clean_type = data_type.trim_end_matches('\0');
            
            // Handle text metadata from chunk data
            if let Ok(text) = std::str::from_utf8(data) {
                let text = text.trim();
                if text.is_empty() {
                    return false;
                }
                
                match clean_type {
                    "ssncasar" => {
                        song.artist = Some(text.to_string());
                        true
                    }
                    "ssncasal" => {
                        song.album = Some(text.to_string());
                        true
                    }
                    "ssncastn" => {
                        song.title = Some(text.to_string());
                        true
                    }
                    "ssncascp" => {
                        song.composer = Some(text.to_string());
                        true
                    }
                    "ssncasaa" => {
                        song.album_artist = Some(text.to_string());
                        true
                    }
                    "ssncasgn" => {
                        song.genre = Some(text.to_string());
                        if !song.genres.contains(&text.to_string()) {
                            song.genres.push(text.to_string());
                        }
                        true
                    }
                    "ssncasdt" => {
                        // Try to parse as year
                        if let Ok(year) = text.parse::<i32>() {
                            song.year = Some(year);
                            true
                        } else {
                            false
                        }
                    }
                    _ => {
                        // Store other text metadata, but filter out internal protocol messages
                        if clean_type.starts_with("ssnc") && !text.is_empty() {
                            let key = if clean_type.len() > 4 { &clean_type[4..] } else { clean_type };
                            
                            // Filter out internal protocol messages
                            let should_skip = matches!(key,
                                "pcst" | "pcen" | "mdst" | "mden" | // Picture/metadata start/end
                                "dapo" | "daid" | "acre" | "prgr" | // DACP and progress
                                "snua" | "cdid" | "cmod" | "cmac" | // Client info
                                "phbt" | "phb0" | "styp" | "flsr" | "pfls" | // Frame/stream info
                                "disc" | "conn" | "clip" | "svip" | "snam" // Connection info
                            );
                            
                            if !should_skip {
                                song.metadata.insert(key.to_string(), serde_json::Value::String(text.to_string()));
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                }
            } else {
                // Handle binary metadata (like track numbers)
                match clean_type {
                    "ssncastn" => {
                        // Track number - binary u16
                        if data.len() >= 2 {
                            let track_num = u16::from_be_bytes([data[0], data[1]]);
                            song.track_number = Some(track_num as i32);
                            true
                        } else {
                            false
                        }
                    }
                    "ssncastc" => {
                        // Track count - binary u16
                        if data.len() >= 2 {
                            let track_count = u16::from_be_bytes([data[0], data[1]]);
                            song.total_tracks = Some(track_count as i32);
                            true
                        } else {
                            false
                        }
                    }
                    _ => {
                        // Store binary metadata as base64, but filter out internal protocol messages
                        if clean_type.starts_with("ssnc") && !data.is_empty() {
                            let key = if clean_type.len() > 4 { &clean_type[4..] } else { clean_type };
                            
                            // Filter out internal protocol messages
                            let should_skip = matches!(key,
                                "pcst" | "pcen" | "mdst" | "mden" | // Picture/metadata start/end
                                "dapo" | "daid" | "acre" | "prgr" | // DACP and progress
                                "snua" | "cdid" | "cmod" | "cmac" | // Client info
                                "phbt" | "phb0" | "styp" | "flsr" | "pfls" | // Frame/stream info
                                "disc" | "conn" | "clip" | "svip" | "snam" // Connection info
                            );
                            
                            if !should_skip {
                                let base64_data = general_purpose::STANDARD.encode(data);
                                song.metadata.insert(
                                    format!("{}_binary", key), 
                                    serde_json::Value::String(base64_data)
                                );
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                }
            }
        }
        ShairportMessage::CompletePicture { data: _, format: _ } => {
            // Artwork has been processed and assembled, but we don't store it in the song
            // The listener can still show the assembled artwork information
            true
        }
        _ => false
    }
}

/// Check if a Song has significant metadata (title, artist, or album)
pub fn song_has_significant_metadata(song: &Song) -> bool {
    song.title.is_some() || song.artist.is_some() || song.album.is_some()
}

/// Display a formatted representation of the song metadata
pub fn display_song_metadata(song: &Song) {
    println!("♪ Current Track:");
    println!("  ┌─────────────────────────────────────────────");
    
    if let Some(title) = &song.title {
        println!("  │ Title:     {}", title);
    }
    if let Some(artist) = &song.artist {
        println!("  │ Artist:    {}", artist);
    }
    if let Some(album) = &song.album {
        println!("  │ Album:     {}", album);
    }
    if let Some(album_artist) = &song.album_artist {
        println!("  │ Album Artist: {}", album_artist);
    }
    if let Some(composer) = &song.composer {
        println!("  │ Composer:  {}", composer);
    }
    if let Some(genre) = &song.genre {
        println!("  │ Genre:     {}", genre);
    }
    if let Some(year) = song.year {
        println!("  │ Year:      {}", year);
    }
    if let Some(track_number) = song.track_number {
        if let Some(total_tracks) = song.total_tracks {
            println!("  │ Track:     {}/{}", track_number, total_tracks);
        } else {
            println!("  │ Track:     {}", track_number);
        }
    }
    if let Some(duration) = song.duration {
        let minutes = (duration / 60.0) as i32;
        let seconds = (duration % 60.0) as i32;
        println!("  │ Duration:  {}:{:02}", minutes, seconds);
    }
    
    // Display additional metadata
    for (key, value) in &song.metadata {
        if let Some(str_value) = value.as_str() {
            // Filter out internal protocol messages from display
            let should_skip = matches!(key.as_str(),
                "PICTURE_START" | "PICTURE_END" | "METADATA_START" | "METADATA_END" | 
                "ITEM_ID" | "SONG_DATA_KIND" | "FRAME_SEQUENCE_REFERENCE" | 
                "PREVIOUS_FRAME_SEQUENCE" | "CLIENT_IP" | "SERVER_IP" | "SERVER_NAME" |
                "DACP_PORT" | "DACP_ID" | "ACTIVE_REMOTE" | "PROGRESS" | "USER_AGENT" |
                "CLIENT_DEVICE_ID" | "CLIENT_MODEL" | "CLIENT_MAC" | "FRAME_POSITION" |
                "FIRST_FRAME_POSITION" | "STREAM_TYPE" | "SONG_TIME_MS" | "CAPABILITIES" |
                "DISCOVERED" | "CONNECTED" | "SORT_NAME" | "SORT_ARTIST" | "SORT_ALBUM" | "SORT_COMPOSER"
            );
            
            if !str_value.is_empty() && !key.ends_with("_binary") && !should_skip {
                println!("  │ {}:  {}", key, str_value);
            }
        }
    }
    
    println!("  └─────────────────────────────────────────────");
    println!();
}

/// Structure to handle chunked UDP messages from Shairport-Sync
/// These are used when large data (like images) exceed UDP packet size limits
#[derive(Debug)]
pub struct ChunkedUdpCollector {
    chunk_collectors: HashMap<u32, ChunkCollector>, // packet_tag -> collector
}

impl Default for ChunkedUdpCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkedUdpCollector {
    pub fn new() -> Self {
        Self {
            chunk_collectors: HashMap::new(),
        }
    }
    
    /// Process a chunked UDP packet and return complete data if all chunks are received
    /// Returns (packet_tag, complete_data) if a complete message is assembled
    pub fn process_chunked_packet(&mut self, buffer: &[u8], bytes_received: usize) -> Option<(u32, Vec<u8>)> {
        // Check if this is a chunked message: minimum size and "ssncchnk" header
        if bytes_received < 24 || &buffer[0..8] != b"ssncchnk" {
            return None;
        }
        
        // Parse chunked message
        let chunk_ix = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
        let chunk_total = u32::from_be_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]);
        let packet_tag = u32::from_be_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
        let _packet_type = u32::from_be_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
        
        let chunk_data = &buffer[24..bytes_received];
        
        // Get or create collector for this packet tag
        self.chunk_collectors.entry(packet_tag).or_insert_with(|| ChunkCollector::new(chunk_total, format!("tag_{:08x}", packet_tag)));
        
        if let Some(collector) = self.chunk_collectors.get_mut(&packet_tag) {
            if let Some(complete_data) = collector.add_chunk(chunk_ix + 1, chunk_data.to_vec()) {
                // Remove the collector since we're done with it
                self.chunk_collectors.remove(&packet_tag);
                return Some((packet_tag, complete_data));
            }
        }
        
        None
    }
    
    /// Check if a buffer contains a chunked UDP message
    pub fn is_chunked_message(buffer: &[u8], bytes_received: usize) -> bool {
        bytes_received >= 24 && &buffer[0..8] == b"ssncchnk"
    }
    
    /// Get packet tag from chunked message (for filtering)
    pub fn get_packet_tag(buffer: &[u8], bytes_received: usize) -> Option<u32> {
        if Self::is_chunked_message(buffer, bytes_received) {
            Some(u32::from_be_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]))
        } else {
            None
        }
    }
}
