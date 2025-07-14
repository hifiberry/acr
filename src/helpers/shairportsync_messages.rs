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
            b"ssncpcst" => {
                if let Ok(session_id) = std::str::from_utf8(payload) {
                    return ShairportMessage::SessionStart(session_id.to_string());
                }
            },
            b"ssncpcen" => {
                if let Ok(timestamp) = std::str::from_utf8(payload) {
                    return ShairportMessage::SessionEnd(timestamp.to_string());
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
            b"ssnccdid" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_DEVICE_ID: {}", content));
                }
            },
            b"ssnccmod" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_MODEL: {}", content));
                }
            },
            b"ssnccmac" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("CLIENT_MAC: {}", content));
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
            
            // Metadata (UTF-8 payload)
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
            b"coreascp" => return ShairportMessage::Control("COMPOSER: (empty)".to_string()),
            b"coreasgn" => return ShairportMessage::Control("GENRE: (empty)".to_string()),
            
            // Client/session information (UTF-8 payload)
            b"ssncsnua" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("USER_AGENT: {}", content));
                }
            },
            b"ssncacre" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("ACTIVE_REMOTE: {}", content));
                }
            },
            b"ssncdaid" => {
                if let Ok(content) = std::str::from_utf8(payload) {
                    return ShairportMessage::Control(format!("DEVICE_ID: {}", content));
                }
            },
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
            b"coremper" => {
                // Core MPEG duration/time - extract the 4-byte timestamp at the end
                if payload.len() >= 4 {
                    let timestamp = u32::from_be_bytes([payload[payload.len()-4], payload[payload.len()-3], payload[payload.len()-2], payload[payload.len()-1]]);
                    return ShairportMessage::Control(format!("MPEG_DURATION: {}", timestamp));
                }
            },
            b"corecaps" => {
                // Core capabilities - usually 1 byte value at the end
                if !payload.is_empty() {
                    let capability = payload[0];
                    return ShairportMessage::Control(format!("CAPABILITIES: {}", capability));
                }
            },
            b"coreastm" => {
                // Core MPEG start time - extract the 4-byte timestamp at the end
                if payload.len() >= 4 {
                    let timestamp = u32::from_be_bytes([payload[payload.len()-4], payload[payload.len()-3], payload[payload.len()-2], payload[payload.len()-1]]);
                    return ShairportMessage::Control(format!("MPEG_START_TIME: {}", timestamp));
                }
            },
            b"coreastn" => {
                // Core track number - extract the value after null bytes
                if payload.len() >= 2 {
                    let track_num = u16::from_be_bytes([payload[0], payload[1]]);
                    return ShairportMessage::Control(format!("TRACK_NUMBER: {}", track_num));
                }
            },
            _ => {}
        }
    }
    
    // Try to parse as UTF-8 text for shorter messages or unknown formats
    if let Ok(text) = std::str::from_utf8(data) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return ShairportMessage::Control(format!("UNKNOWN_TEXT: {}", trimmed));
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

pub fn print_hex_dump(data: &[u8], prefix: &str) {
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("{}{:04x}: ", prefix, i * 16);
        
        // Print hex values
        for (j, byte) in chunk.iter().enumerate() {
            print!("{:02x} ", byte);
            if j == 7 {
                print!(" "); // Extra space in the middle
            }
        }
        
        // Pad if this chunk is less than 16 bytes
        for j in chunk.len()..16 {
            print!("   ");
            if j == 7 {
                print!(" ");
            }
        }
        
        print!(" |");
        
        // Print ASCII representation
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                print!("{}", *byte as char);
            } else {
                print!(".");
            }
        }
        
        println!("|");
    }
}

pub fn display_shairport_message(message: &ShairportMessage, show_hex: bool) {
    match message {
        ShairportMessage::Control(action) => {
            println!("  {}", action);
        }
        
        ShairportMessage::SessionStart(session_id) => {
            println!("  SESSION START: {}", session_id);
        }
        
        ShairportMessage::SessionEnd(timestamp) => {
            println!("  SESSION END: {}", timestamp);
        }
        
        ShairportMessage::CompletePicture { data, format } => {
            println!("  COMPLETE PICTURE:");
            println!("     Format: {}", format);
            println!("     Size: {} bytes", data.len());
            println!("     Dimensions: {}", get_image_dimensions(data, format));
            
            if show_hex && data.len() <= 256 {
                println!("     Hex dump (header):");
                print_hex_dump(data, "       ");
            } else if show_hex {
                println!("     Hex dump (first 256 bytes):");
                print_hex_dump(&data[..256], "       ");
            }
        }
        
        ShairportMessage::ChunkData { chunk_id, total_chunks, data_type, data } => {
            println!("  CHUNK DATA:");
            println!("     Type: {}", data_type.trim_end_matches('\0'));
            println!("     Chunk: {}/{}", chunk_id, total_chunks);
            
            if data.is_empty() {
                println!("     Size: 0 bytes (header/padding only)");
            } else {
                println!("     Size: {} bytes", data.len());
            }
            
            // Special handling for different data types
            let clean_type = data_type.trim_end_matches('\0');
            match clean_type {
                "ssncPICT" => {
                    if data.is_empty() {
                        println!("     Content: Album artwork header (no data in this chunk)");
                    } else {
                        let format = detect_image_format(data);
                        println!("     Content: Album artwork ({})", format);
                        println!("     Format: {} detected", format);
                        
                        if format.contains("JPEG") {
                            let dimensions = get_jpeg_dimensions(data);
                            if dimensions != "Unknown" {
                                println!("     Dimensions: {}", dimensions);
                            }
                        } else if format.contains("PNG") {
                            let dimensions = get_png_dimensions(data);
                            if dimensions != "Unknown" {
                                println!("     Dimensions: {}", dimensions);
                            }
                        }
                    }
                }
                "ssncminu" => println!("     Content: Metadata - Track info"),
                "ssncasar" => println!("     Content: Metadata - Artist"),
                "ssncasal" => println!("     Content: Metadata - Album"),
                "ssncastn" => println!("     Content: Metadata - Track name"),
                _ => {
                    if clean_type.starts_with("ssnc") {
                        println!("     Content: Metadata - {}", &clean_type[4..]);
                    } else {
                        println!("     Content: Unknown data type");
                    }
                }
            }
            
            if !data.is_empty() && show_hex {
                if data.len() <= 256 {
                    println!("     Hex dump:");
                    print_hex_dump(data, "       ");
                } else {
                    println!("     Hex dump (first 256 bytes):");
                    print_hex_dump(&data[..256], "       ");
                }
            }
        }
        
        ShairportMessage::Unknown(data) => {
            // Try to display as text if it looks like text
            if let Ok(text) = std::str::from_utf8(data) {
                if text.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) {
                    println!("  UNKNOWN TEXT: {}", text.trim());
                    return;
                }
            }
            
            println!("  UNKNOWN BINARY DATA: {} bytes", data.len());
            print_hex_dump(data, "     ");
        }
    }
}
