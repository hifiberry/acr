#![cfg(unix)]

use clap::Parser;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

use audiocontrol::helpers::shairportsync_messages::{
    ShairportMessage, ChunkCollector, parse_shairport_message, 
    detect_image_format, get_image_dimensions, get_jpeg_dimensions, get_png_dimensions,
    update_song_from_message, song_has_significant_metadata, display_song_metadata
};
use audiocontrol::data::song::Song;

#[derive(Parser)]
#[command(name = "audiocontrol_listen_shairportsync")]
#[command(about = "AudioControl ShairportSync UDP Listener")]
#[command(long_about = "Listens for UDP packets on the specified port and displays their content.\n\nModes:\n- full: Shows all packets with detailed information (default)\n- player: Collects metadata and displays structured song information\n- dump: Saves packets to binary file with relative timestamps for later analysis\n\nThis tool is useful for monitoring ShairportSync metadata or other\nUDP-based communication. Press Ctrl+C to stop listening.")]
#[command(version)]
struct Args {
    /// UDP port to listen on
    #[arg(long, default_value_t = 5555)]
    port: u16,
    
    /// Show raw hex dump for binary data
    #[arg(long, default_value_t = false)]
    show_hex: bool,
    
    /// Display mode: full (all packets) or player (structured metadata)
    #[arg(long, value_enum, default_value_t = DisplayMode::Full)]
    mode: DisplayMode,
    
    /// Output file for dump mode (default: shairport_dump.bin)
    #[arg(long, default_value = "shairport_dump.bin")]
    output_file: String,
    
    /// Save cover art to file (default: coverart.EXTENSION, empty = don't save)
    #[arg(long, default_value = "coverart")]
    save_coverart: String,
}

#[derive(Clone, clap::ValueEnum, PartialEq)]
enum DisplayMode {
    /// Show all packets with detailed information
    Full,
    /// Collect metadata and display structured song information
    Player,
    /// Dump packets to file with timestamps for later use in tests
    Dump,
}

fn main() {
    env_logger::init();
    
    let args = Args::parse();
    let port = args.port;
    let show_hex = args.show_hex;
    let mode = args.mode;
    let save_coverart = args.save_coverart.clone();
    
    println!("AudioControl ShairportSync UDP Listener");
    println!("=====================================");
    println!("Listening on UDP port: {}", port);
    match mode {
        DisplayMode::Full => println!("Mode: Full (showing all packets)"),
        DisplayMode::Player => println!("Mode: Player (structured metadata display)"),
        DisplayMode::Dump => println!("Mode: Dump (saving to file: {})", args.output_file),
    }
    println!("Press Ctrl+C to stop...");
    println!();
    
    // Set up signal handler for Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    if let Err(e) = ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }) {
        eprintln!("Error: Failed to set Ctrl+C handler: {}", e);
        std::process::exit(1);
    }
    
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
    if let Err(e) = socket.set_read_timeout(Some(std::time::Duration::from_millis(1000))) {
        eprintln!("Error: Failed to set socket timeout: {}", e);
        std::process::exit(1);
    }
    
    let mut buffer = [0; 4096]; // 4KB buffer for incoming packets
    let mut packet_count = 0;
    let mut picture_collector: Option<ChunkCollector> = None;
    let mut current_song = Song::default();
    let mut metadata_updated = false;
    
    // Initialize dump file writer if in dump mode
    let mut dump_writer = if mode == DisplayMode::Dump {
        match File::create(&args.output_file) {
            Ok(file) => {
                println!("Created dump file: {}", args.output_file);
                Some(BufWriter::new(file))
            }
            Err(e) => {
                eprintln!("Error: Failed to create dump file {}: {}", args.output_file, e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    
    let start_time = Instant::now();
    
    while running.load(Ordering::SeqCst) {
        match socket.recv_from(&mut buffer) {
            Ok((bytes_received, sender_addr)) => {
                packet_count += 1;
                
                // Parse ShairportSync message
                let mut message = parse_shairport_message(&buffer[..bytes_received]);
                
                // Handle chunk collection for pictures and binary data
                if let ShairportMessage::ChunkData { chunk_id, total_chunks, data_type, data } = &message {
                    let clean_type = data_type.trim_end_matches('\0');
                    
                    // Check if this might be picture data by looking at the data content or type
                    let is_picture_data = clean_type == "ssncPICT" || 
                                         clean_type.contains("PICT") ||
                                         (!data.is_empty() && is_likely_image_data(data));
                    
                    if is_picture_data && *total_chunks > 1 {
                        // Initialize collector if this is the first chunk or we don't have one
                        if picture_collector.is_none() || 
                           picture_collector.as_ref().unwrap().total_chunks != *total_chunks {
                            picture_collector = Some(ChunkCollector::new(*total_chunks, clean_type.to_string()));
                        }
                        
                        // Add chunk to collector
                        if let Some(ref mut collector) = picture_collector {
                            if let Some(complete_data) = collector.add_chunk(*chunk_id, data.clone()) {
                                // We have a complete picture
                                let format = detect_image_format(&complete_data);
                                let dimensions = get_image_dimensions(&complete_data, &format);
                                if mode == DisplayMode::Player {
                                    println!("📷 Assembled complete artwork: {} ({} bytes, {})", 
                                            format, complete_data.len(), dimensions);
                                }
                                
                                // Save cover art if requested
                                if !save_coverart.is_empty() {
                                    match save_coverart_to_file(&complete_data, &format, &save_coverart) {
                                        Ok(filename) => {
                                            println!("💾 Cover art saved to: {}", filename);
                                        }
                                        Err(e) => {
                                            eprintln!("❌ Failed to save cover art: {}", e);
                                        }
                                    }
                                }
                                
                                message = ShairportMessage::CompletePicture {
                                    data: complete_data,
                                    format,
                                };
                                picture_collector = None; // Reset for next picture
                            }
                        }
                    }
                }
                
                // Handle chunked UDP messages (for large images that exceed UDP size limits)
                // Format: "ssnc", "chnk", packet_ix, packet_counts, packet_tag, packet_type, chunked_data
                if bytes_received >= 24 {  // minimum size for chunked message
                    if &buffer[0..8] == b"ssncchnk" {
                        // Parse chunked message
                        let chunk_ix = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
                        let chunk_total = u32::from_be_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]);
                        let packet_tag = u32::from_be_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
                        let packet_type = u32::from_be_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
                        
                        let chunk_data = &buffer[24..bytes_received];
                        
                        if mode == DisplayMode::Full {
                            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                            println!("[{}] Chunked UDP packet #{}: chunk {}/{}, tag: {}, type: {}, data: {} bytes", 
                                     timestamp, packet_count, chunk_ix + 1, chunk_total, packet_tag, packet_type, chunk_data.len());
                        }
                        
                        // Handle PICT chunked data
                        if packet_tag == 0x50494354 { // "PICT" in big-endian
                            // Initialize collector if this is the first chunk or we don't have one
                            if picture_collector.is_none() || 
                               picture_collector.as_ref().unwrap().total_chunks != chunk_total {
                                picture_collector = Some(ChunkCollector::new(chunk_total, "PICT".to_string()));
                            }
                            
                            // Add chunk to collector
                            if let Some(ref mut collector) = picture_collector {
                                if let Some(complete_data) = collector.add_chunk(chunk_ix + 1, chunk_data.to_vec()) {
                                    // We have a complete picture from chunked UDP
                                    let format = detect_image_format(&complete_data);
                                    let dimensions = get_image_dimensions(&complete_data, &format);
                                    
                                    if mode == DisplayMode::Player {
                                        println!("📷 Assembled complete artwork from UDP chunks: {} ({} bytes, {})", 
                                                format, complete_data.len(), dimensions);
                                    }
                                    
                                    // Save cover art if requested
                                    if !save_coverart.is_empty() {
                                        match save_coverart_to_file(&complete_data, &format, &save_coverart) {
                                            Ok(filename) => {
                                                println!("💾 Cover art saved to: {}", filename);
                                            }
                                            Err(e) => {
                                                eprintln!("❌ Failed to save cover art: {}", e);
                                            }
                                        }
                                    }
                                    
                                    // Complete picture assembled but not processed in this path
                                    // since we continue to next iteration
                                    picture_collector = None; // Reset for next picture
                                }
                            }
                            continue; // Skip normal processing for chunked messages
                        }
                    }
                }
                
                match mode {
                    DisplayMode::Full => {
                        // Get current timestamp
                        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                        
                        println!("[{}] Packet #{} from {} ({} bytes):", 
                                 timestamp, packet_count, sender_addr, bytes_received);
                        
                        display_shairport_message(&message, show_hex);
                        
                        // Save cover art if it's a complete picture and save_coverart is specified
                        if !save_coverart.is_empty() {
                            if let ShairportMessage::CompletePicture { data, format } = &message {
                                match save_coverart_to_file(data, format, &save_coverart) {
                                    Ok(filename) => {
                                        println!("💾 Cover art saved to: {}", filename);
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Failed to save cover art: {}", e);
                                    }
                                }
                            }
                        }
                        
                        println!(); // Empty line between packets
                    }
                    DisplayMode::Player => {
                        // Update metadata and show player events
                        let updated = update_song_from_message(&mut current_song, &message);
                        if updated {
                            metadata_updated = true;
                        }
                        
                        // Show control events and unknown messages immediately
                        match &message {
                            ShairportMessage::Control(action) => {
                                let timestamp = chrono::Local::now().format("%H:%M:%S");
                                // Filter out metadata messages that we're handling separately
                                if !action.contains(": ") || action.starts_with("PAUSE") || 
                                   action.starts_with("RESUME") || action.starts_with("SESSION") ||
                                   action.starts_with("PLAYBACK") || action.starts_with("AUDIO") ||
                                   action.starts_with("VOLUME") || action.starts_with("PROGRESS") {
                                    println!("[{}] ♫ {}", timestamp, action);
                                }
                            }
                            ShairportMessage::SessionStart(session_id) => {
                                let timestamp = chrono::Local::now().format("%H:%M:%S");
                                println!("[{}] 🎵 Session started: {}", timestamp, session_id);
                                // Clear previous metadata on new session
                                current_song = Song::default();
                                metadata_updated = false;
                            }
                            ShairportMessage::SessionEnd(timestamp_str) => {
                                let timestamp = chrono::Local::now().format("%H:%M:%S");
                                println!("[{}] 🎵 Session ended: {}", timestamp, timestamp_str);
                                // Show final metadata if we have any
                                if song_has_significant_metadata(&current_song) {
                                    display_song_metadata(&current_song);
                                }
                                current_song = Song::default();
                                metadata_updated = false;
                            }
                            ShairportMessage::Unknown(data) => {
                                let timestamp = chrono::Local::now().format("%H:%M:%S");
                                if let Ok(text) = std::str::from_utf8(data) {
                                    if text.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) {
                                        println!("[{}] ❓ Unknown text: {}", timestamp, text.trim());
                                    } else {
                                        println!("[{}] ❓ Unknown binary data: {} bytes", timestamp, data.len());
                                    }
                                } else {
                                    println!("[{}] ❓ Unknown binary data: {} bytes", timestamp, data.len());
                                }
                            }
                            _ => {
                                // For metadata messages, we've already updated the current_song
                                // Display updated metadata when we have significant changes
                                if metadata_updated && song_has_significant_metadata(&current_song) {
                                    display_song_metadata(&current_song);
                                    metadata_updated = false;
                                }
                            }
                        }
                    }
                    DisplayMode::Dump => {
                        // Write packet to dump file with relative timestamp
                        if let Some(ref mut writer) = dump_writer {
                            let relative_time_ms = start_time.elapsed().as_millis() as u64;
                            
                            // Write header: timestamp (8 bytes) + packet_size (4 bytes)
                            if let Err(e) = writer.write_all(&relative_time_ms.to_le_bytes()) {
                                eprintln!("Error writing timestamp to dump file: {}", e);
                                break;
                            }
                            if let Err(e) = writer.write_all(&(bytes_received as u32).to_le_bytes()) {
                                eprintln!("Error writing packet size to dump file: {}", e);
                                break;
                            }
                            
                            // Write the actual packet data
                            if let Err(e) = writer.write_all(&buffer[..bytes_received]) {
                                eprintln!("Error writing packet data to dump file: {}", e);
                                break;
                            }
                            
                            // Flush periodically to ensure data is written
                            if packet_count % 100 == 0 {
                                if let Err(e) = writer.flush() {
                                    eprintln!("Error flushing dump file: {}", e);
                                    break;
                                }
                            }
                            
                            // Print progress every 1000 packets
                            if packet_count % 1000 == 0 {
                                println!("Dumped {} packets to {}", packet_count, args.output_file);
                            }
                        }
                    }
                }
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
        }    }
    
    // Flush and close dump file if in dump mode
    if let Some(mut writer) = dump_writer {
        if let Err(e) = writer.flush() {
            eprintln!("Error flushing dump file on exit: {}", e);
        }
        println!("Dump file {} closed.", args.output_file);
    }

    println!("Listener stopped. Total packets received: {}", packet_count);
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

fn display_shairport_message(message: &ShairportMessage, show_hex: bool) {
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
                    if let Some(suffix) = clean_type.strip_prefix("ssnc") {
                        println!("     Content: Metadata - {}", suffix);
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
            // Try to display as text if it looks like text, but always show hex dump
            if let Ok(text) = std::str::from_utf8(data) {
                if text.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) {
                    println!("  UNKNOWN TEXT: {}", text.trim());
                    println!("  Hex dump:");
                    print_hex_dump(data, "     ");
                    return;
                }
            }
            
            println!("  UNKNOWN BINARY DATA: {} bytes", data.len());
            print_hex_dump(data, "     ");
        }
    }
}

/// Check if data is likely to be image data based on magic bytes
fn is_likely_image_data(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    
    // Check for common image format magic bytes
    match &data[0..4] {
        [0xFF, 0xD8, 0xFF, _] => true,           // JPEG
        [0x89, 0x50, 0x4E, 0x47] => true,        // PNG
        [0x47, 0x49, 0x46, 0x38] => true,        // GIF
        [0x42, 0x4D, _, _] => true,              // BMP
        _ => {
            // Check for other formats
            if data.len() >= 12 && &data[4..12] == b"ftypheic" {
                true  // HEIC
            } else if data.len() >= 8 && &data[0..8] == b"RIFF" {
                true  // WEBP
            } else {
                false
            }
        }
    }
}

/// Save cover art to file with appropriate extension
fn save_coverart_to_file(data: &[u8], format: &str, base_filename: &str) -> Result<String, std::io::Error> {
    if base_filename.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Empty filename provided"
        ));
    }
    
    // Determine file extension based on format
    let extension = match format.to_lowercase().as_str() {
        format_str if format_str.contains("jpeg") || format_str.contains("jpg") => "jpg",
        format_str if format_str.contains("png") => "png",
        format_str if format_str.contains("gif") => "gif",
        format_str if format_str.contains("bmp") => "bmp",
        format_str if format_str.contains("webp") => "webp",
        format_str if format_str.contains("heic") => "heic",
        _ => "bin", // fallback for unknown formats
    };
    
    let filename = format!("{}.{}", base_filename, extension);
    
    // Write the data to file
    std::fs::write(&filename, data)?;
    
    Ok(filename)
}
