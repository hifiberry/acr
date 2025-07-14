use std::collections::HashMap;

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
    total_chunks: u32,
    data_type: String,
}

impl ChunkCollector {
    pub fn new(total_chunks: u32, data_type: String) -> Self {
        Self {
            chunks: HashMap::new(),
            total_chunks,
            data_type,
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
            if marker >= 0xC0 && marker <= 0xC3 {
                if i + 9 < data.len() {
                    let height = u16::from_be_bytes([data[i + 5], data[i + 6]]);
                    let width = u16::from_be_bytes([data[i + 7], data[i + 8]]);
                    return format!("{}x{}", width, height);
                }
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
