#![cfg(unix)]

use clap::Parser;
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "audiocontrol_listen_shairportsync")]
#[command(about = "AudioControl ShairportSync UDP Listener")]
#[command(long_about = "Listens for UDP packets on the specified port and displays their content.\nPackets are assumed to be text and will be displayed as such. If binary\ndata is received, it will be shown as a hex dump.\n\nThis tool is useful for monitoring ShairportSync metadata or other\nUDP-based communication. Press Ctrl+C to stop listening.")]
#[command(version)]
struct Args {
    /// UDP port to listen on
    #[arg(long, default_value_t = 5555)]
    port: u16,
    
    /// Show raw hex dump for binary data
    #[arg(long, default_value_t = false)]
    show_hex: bool,
}

#[derive(Debug)]
enum ShairportMessage {
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
struct ChunkCollector {
    chunks: HashMap<u32, Vec<u8>>, // chunk_id -> data
    total_chunks: u32,
}

impl ChunkCollector {
    fn new(total_chunks: u32, _data_type: String) -> Self {
        Self {
            chunks: HashMap::new(),
            total_chunks,
        }
    }
    
    fn add_chunk(&mut self, chunk_id: u32, data: Vec<u8>) -> Option<Vec<u8>> {
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

fn main() {
    env_logger::init();
    
    let args = Args::parse();
    let port = args.port;
    let show_hex = args.show_hex;
    
    println!("AudioControl ShairportSync UDP Listener");
    println!("=====================================");
    println!("Listening on UDP port: {}", port);
    println!("Press Ctrl+C to stop...");
    println!();
    
    // Set up signal handler for Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");
    
    // Bind to UDP socket
    let bind_address = format!("0.0.0.0:{}", port);
    let socket = match UdpSocket::bind(&bind_address) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Failed to bind to {}: {}", bind_address, e);
            std::process::exit(1);
        }
    };
    
    println!("Successfully bound to {}", bind_address);
    println!("Waiting for packets...");
    println!();
    
    // Set socket timeout to allow checking the running flag
    socket.set_read_timeout(Some(std::time::Duration::from_millis(1000)))
        .expect("Failed to set socket timeout");
    
    let mut buffer = [0; 4096]; // 4KB buffer for incoming packets
    let mut packet_count = 0;
    let mut picture_collector: Option<ChunkCollector> = None;
    
    while running.load(Ordering::SeqCst) {
        match socket.recv_from(&mut buffer) {
            Ok((bytes_received, sender_addr)) => {
                packet_count += 1;
                
                // Get current timestamp
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                
                println!("[{}] Packet #{} from {} ({} bytes):", 
                         timestamp, packet_count, sender_addr, bytes_received);
                
                // Parse ShairportSync message
                let mut message = parse_shairport_message(&buffer[..bytes_received]);
                
                // Handle chunk collection for pictures
                if let ShairportMessage::ChunkData { chunk_id, total_chunks, data_type, data } = &message {
                    let clean_type = data_type.trim_end_matches('\0');
                    
                    if clean_type == "ssncPICT" {
                        // Initialize collector if this is the first chunk
                        if picture_collector.is_none() {
                            picture_collector = Some(ChunkCollector::new(*total_chunks, clean_type.to_string()));
                        }
                        
                        // Add chunk to collector
                        if let Some(ref mut collector) = picture_collector {
                            if let Some(complete_data) = collector.add_chunk(*chunk_id, data.clone()) {
                                // We have a complete picture
                                let format = detect_image_format(&complete_data);
                                message = ShairportMessage::CompletePicture {
                                    data: complete_data,
                                    format,
                                };
                                picture_collector = None; // Reset for next picture
                            }
                        }
                    }
                }
                
                display_shairport_message(&message, show_hex);
                
                println!(); // Empty line between packets
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                        // Timeout occurred, continue loop to check running flag
                        continue;
                    }
                    _ => {
                        eprintln!("Error receiving packet: {}", e);
                        break;
                    }
                }
            }
        }
    }
    
    println!("Listener stopped. Total packets received: {}", packet_count);
}

fn detect_image_format(data: &[u8]) -> String {
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

fn parse_shairport_message(data: &[u8]) -> ShairportMessage {
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
    
    // Try to parse as UTF-8 text
    if let Ok(text) = std::str::from_utf8(data) {
        let trimmed = text.trim();
        
        // Control messages
        match trimmed {
            "ssncpaus" => return ShairportMessage::Control("PAUSE".to_string()),
            "ssncpres" => return ShairportMessage::Control("RESUME".to_string()),
            "ssncaend" => return ShairportMessage::Control("SESSION_END".to_string()),
            "ssncabeg" => return ShairportMessage::Control("AUDIO_BEGIN".to_string()),
            "ssncpbeg" => return ShairportMessage::Control("PLAYBACK_BEGIN".to_string()),
            "ssncPICT" => return ShairportMessage::Control("PICTURE_REQUEST".to_string()),
            _ => {
                // Check for session start/end with IDs
                if trimmed.starts_with("ssncpcst") {
                    let session_id = &trimmed[8..]; // Remove "ssncpcst" prefix
                    return ShairportMessage::SessionStart(session_id.to_string());
                } else if trimmed.starts_with("ssncpcen") {
                    let timestamp = &trimmed[8..]; // Remove "ssncpcen" prefix
                    return ShairportMessage::SessionEnd(timestamp.to_string());
                }
                
                // Parse other ShairportSync messages
                if trimmed.len() >= 8 {
                    let prefix = &trimmed[0..8];
                    let content = &trimmed[8..];
                    
                    return match prefix {
                        // Connection info
                        "ssncdisc" => ShairportMessage::Control(format!("DISCOVERED: {}", content)),
                        "ssncconn" => ShairportMessage::Control(format!("CONNECTED: {}", content)),
                        "ssncclip" => ShairportMessage::Control(format!("CLIENT_IP: {}", content)),
                        "ssncsvip" => ShairportMessage::Control(format!("SERVER_IP: {}", content)),
                        "ssncsnam" => ShairportMessage::Control(format!("SERVER_NAME: {}", content)),
                        "ssnccdid" => ShairportMessage::Control(format!("CLIENT_DEVICE_ID: {}", content)),
                        "ssnccmod" => ShairportMessage::Control(format!("CLIENT_MODEL: {}", content)),
                        "ssnccmac" => ShairportMessage::Control(format!("CLIENT_MAC: {}", content)),
                        
                        // Playback control
                        "ssncpvol" => ShairportMessage::Control(format!("VOLUME: {}", content)),
                        "ssncprgr" => ShairportMessage::Control(format!("PROGRESS: {}", content)),
                        "ssncmdst" => ShairportMessage::Control(format!("METADATA_START: {}", content)),
                        "ssncmden" => ShairportMessage::Control(format!("METADATA_END: {}", content)),
                        
                        // Metadata (using 'core' prefix for iTunes-style metadata)
                        "coreasal" => ShairportMessage::Control(format!("ALBUM: {}", content)),
                        "coreasar" => ShairportMessage::Control(format!("ARTIST: {}", content)),
                        "coreminm" => ShairportMessage::Control(format!("TRACK: {}", content)),
                        "coreascp" => ShairportMessage::Control("COMPOSER: (empty)".to_string()),
                        "coreasgn" => ShairportMessage::Control("GENRE: (empty)".to_string()),
                        
                        _ => ShairportMessage::Unknown(data.to_vec()),
                    };
                }
            }
        }
    }
    
    // If nothing else matches, it's unknown
    ShairportMessage::Unknown(data.to_vec())
}

fn display_shairport_message(message: &ShairportMessage, show_hex: bool) {
    match message {
        ShairportMessage::Control(action) => {
            // Use different emojis based on the action type
            let emoji = if action.starts_with("DISCOVERED") || action.starts_with("CONNECTED") {
                "�"
            } else if action.starts_with("CLIENT_") || action.starts_with("SERVER_") {
                "📱"
            } else if action.starts_with("VOLUME") {
                "🔊"
            } else if action.starts_with("PROGRESS") {
                "⏱️"
            } else if action.starts_with("METADATA_") {
                "📋"
            } else if action.starts_with("ALBUM") || action.starts_with("ARTIST") || action.starts_with("TRACK") {
                "🎵"
            } else if action.contains("BEGIN") {
                "▶️"
            } else if action.contains("PAUSE") {
                "⏸️"
            } else if action.contains("RESUME") {
                "▶️"
            } else {
                "📻"
            };
            
            println!("  {} {}", emoji, action);
        }
        
        ShairportMessage::SessionStart(session_id) => {
            println!("  � SESSION START: {}", session_id);
        }
        
        ShairportMessage::SessionEnd(timestamp) => {
            println!("  ⏹️  SESSION END: {}", timestamp);
        }
        
        ShairportMessage::CompletePicture { data, format } => {
            println!("  🖼️  COMPLETE PICTURE:");
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
            println!("  📦 CHUNK DATA:");
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
                    println!("  ❓ UNKNOWN TEXT: {}", text.trim());
                    return;
                }
            }
            
            println!("  ❓ UNKNOWN BINARY DATA: {} bytes", data.len());
            if show_hex {
                print_hex_dump(data, "     ");
            }
        }
    }
}

fn get_image_dimensions(data: &[u8], format: &str) -> String {
    match format {
        "JPEG" => get_jpeg_dimensions(data),
        "PNG" => get_png_dimensions(data),
        _ => "Unknown".to_string(),
    }
}

fn get_jpeg_dimensions(data: &[u8]) -> String {
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

fn get_png_dimensions(data: &[u8]) -> String {
    // PNG IHDR chunk starts at byte 8 and contains width/height at bytes 16-23
    if data.len() >= 24 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        format!("{}x{}", width, height)
    } else {
        "Unknown".to_string()
    }
}

fn print_hex_dump(data: &[u8], prefix: &str) {
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
